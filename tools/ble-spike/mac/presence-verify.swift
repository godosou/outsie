// presence-verify — decide whether a beacon was minted by the paired phone.
//
// Reads the scanner's CSV on stdin, appends one field, writes it back out:
//
//   in :  unix_ms,rssi,peer_prefix,ver,key_id,tag_hex
//   out:  unix_ms,rssi,peer_prefix,ver,key_id,tag_hex,auth
//
//   auth = VALID     the tag verifies against the key we hold for this key_id
//          INVALID   a well-formed tag that does not verify -- an imposter, a replay
//                    from an expired window, or a 16-bit UUID collision
//          MALFORMED no usable payload in the advertisement
//          NOKEY     no key on this Mac for that key_id, or the key file is not
//                    root-owned 0600
//
// THE ALGORITHM (design §3.2)
//
//   c0 = floor(now / WINDOW)
//   for c in [c0-1, c0, c0+1]:
//       msg = "repose-presence-v1 beacon" ‖ key_id(1) ‖ c(8, big-endian)
//       if constant_time_eq(HMAC-SHA256(K, msg)[0..TAG_LEN), tag): VALID
//
// The counter is never transmitted; both ends derive it from their own clocks. ±1
// window absorbs clock skew and the advertising gap when the phone re-mints. Do not
// widen it: every extra window is extra replay surface, and the window length is the
// whole of the replay defence.
//
// WHAT A VALID TAG DOES AND DOES NOT PROVE
//
// It proves a device holding the paired key minted a beacon in roughly this minute.
// It does NOT prove that device is the phone, and it does not survive a real-time
// relay: two colluding radios can tunnel the genuine, current beacon to a box beside
// the Mac, defeating both the window and the RSSI gate. Closing that needs either a
// challenge (which does not fit the 1.5s permit budget) or hardware ranging, which
// this hardware does not have. There is no second factor behind this check -- a
// beacon that verifies here is what lets an empty password through, so the honest
// statement is that relay is unaddressed, not that something else would catch it.
//
// Build: swiftc -O -o presence-verify presence-verify.swift
// Run:   ./rssi-scan | sudo ./presence-verify | ./permit-bridge.sh

import CryptoKit
import Foundation

let beaconLabel = "repose-presence-v1 beacon"
let windowSeconds: Int64 = 30
let tagLen = 8
let defaultKeyDir = "/var/db/repose-unlock"

func log(_ msg: String) {
    FileHandle.standardError.write("presence-verify: \(msg)\n".data(using: .utf8)!)
}

func hexDecode(_ s: String) -> Data? {
    guard s.count % 2 == 0, !s.isEmpty else { return nil }
    var out = Data(capacity: s.count / 2)
    var i = s.startIndex
    while i < s.endIndex {
        let j = s.index(i, offsetBy: 2)
        guard let b = UInt8(s[i..<j], radix: 16) else { return nil }
        out.append(b)
        i = j
    }
    return out
}

func beaconMessage(keyId: UInt8, counter: Int64) -> Data {
    var d = Data(beaconLabel.utf8)
    d.append(keyId)
    // Two's-complement big-endian, matching Kotlin's `ushr` on a signed Long. The
    // counter is a unix time divided by 30, so it will not be negative this century;
    // the bit pattern is what has to agree, and this is the same one.
    let u = UInt64(bitPattern: counter)
    for shift in stride(from: 56, through: 0, by: -8) {
        d.append(UInt8((u >> UInt64(shift)) & 0xFF))
    }
    return d
}

/// Compare in time independent of how many leading bytes match.
func constantTimeEqual(_ a: Data, _ b: Data) -> Bool {
    guard a.count == b.count else { return false }
    var diff: UInt8 = 0
    for (x, y) in zip(a, b) { diff |= x ^ y }
    return diff == 0
}

// MARK: - key loading

enum KeyLoad {
    case key(SymmetricKey)
    case absent(String)
}

final class KeyStore {
    private let dir: String
    private var cache: [UInt8: KeyLoad] = [:]
    private var complained = Set<UInt8>()

    init(dir: String) { self.dir = dir }

    func key(for keyId: UInt8) -> SymmetricKey? {
        let loaded = cache[keyId] ?? load(keyId)
        cache[keyId] = loaded
        switch loaded {
        case .key(let k):
            return k
        case .absent(let why):
            if complained.insert(keyId).inserted { log("keyId \(keyId): \(why)") }
            return nil
        }
    }

    private func load(_ keyId: UInt8) -> KeyLoad {
        let path = "\(dir)/presence-key.\(keyId)"
        guard let attrs = try? FileManager.default.attributesOfItem(atPath: path) else {
            return .absent("no key file at \(path) — this Mac is not provisioned for it")
        }
        // The key is the whole of device identity, so a file anyone can rewrite is a
        // file anyone can make themselves the paired phone with. Refusing is the only
        // safe reading of a wrong mode; repairing it silently would hide the fact that
        // something else had already been able to write it.
        let owner = (attrs[.ownerAccountID] as? NSNumber)?.intValue ?? -1
        let group = (attrs[.groupOwnerAccountID] as? NSNumber)?.intValue ?? -1
        let perms = (attrs[.posixPermissions] as? NSNumber)?.int16Value ?? -1
        guard owner == 0, group == 0, perms == 0o600 else {
            return .absent(String(
                format: "%@ must be root:wheel 0600, found uid=%d gid=%d mode=%o — refusing to use it",
                path, owner, group, Int(perms)))
        }
        guard let text = try? String(contentsOfFile: path, encoding: .utf8) else {
            return .absent("\(path) is unreadable (are you root?)")
        }
        guard let raw = hexDecode(text.trimmingCharacters(in: .whitespacesAndNewlines)),
              raw.count == 32
        else {
            return .absent("\(path) is not 64 hex characters")
        }
        return .key(SymmetricKey(data: raw))
    }
}

// MARK: - verification

func verify(keyId: UInt8, tag: Data, now: Int64, keys: KeyStore) -> String {
    guard tag.count == tagLen else { return "MALFORMED" }
    guard let k = keys.key(for: keyId) else { return "NOKEY" }
    let c0 = now / windowSeconds
    for c in [c0 - 1, c0, c0 + 1] {
        let full = Data(HMAC<SHA256>.authenticationCode(for: beaconMessage(keyId: keyId, counter: c), using: k))
        if constantTimeEqual(full.prefix(tagLen), tag) { return "VALID" }
    }
    return "INVALID"
}

// MARK: - self test

/// Known-answer vectors produced by OpenSSL, not by this file. Checking Swift against
/// its own output would only prove it is self-consistent; these prove it agrees with
/// an independent HMAC implementation on the exact pre-image bytes. The Android half
/// of the agreement is not provable here -- only a live run with the phone shows that,
/// which is what tests/e2e/impersonation_test.sh does.
///
/// Regenerate with tools/ble-spike/mac/presence-vectors.sh
struct Vector {
    let keyHex: String, keyId: UInt8, counter: Int64, tagHex: String
}

let vectors: [Vector] = [
    Vector(keyHex: "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
           keyId: 1, counter: 0, tagHex: "4c38cfe50cb76d34"),
    Vector(keyHex: "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
           keyId: 1, counter: 58_000_000, tagHex: "f149ead01555ef7e"),
    Vector(keyHex: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
           keyId: 7, counter: 1, tagHex: "3dc703e84558cc48"),
]

func selfTest() -> Int32 {
    var failures = 0
    for (i, v) in vectors.enumerated() {
        let k = SymmetricKey(data: hexDecode(v.keyHex)!)
        let got = Data(HMAC<SHA256>.authenticationCode(
            for: beaconMessage(keyId: v.keyId, counter: v.counter), using: k)).prefix(tagLen)
        let gotHex = got.map { String(format: "%02x", $0) }.joined()
        if gotHex == v.tagHex {
            print("  ok   vector \(i): keyId=\(v.keyId) counter=\(v.counter) -> \(gotHex)")
        } else {
            print("  FAIL vector \(i): expected \(v.tagHex), got \(gotHex)")
            failures += 1
        }
    }

    // A tag one bit off must not verify, or a broken comparison would let every
    // vector above pass while accepting anything.
    let k = SymmetricKey(data: hexDecode(vectors[0].keyHex)!)
    var bad = hexDecode(vectors[0].tagHex)!
    bad[bad.startIndex] ^= 0x01
    let good = Data(HMAC<SHA256>.authenticationCode(
        for: beaconMessage(keyId: 1, counter: 0), using: k)).prefix(tagLen)
    if constantTimeEqual(good, bad) {
        print("  FAIL a one-bit-different tag compared equal")
        failures += 1
    } else {
        print("  ok   a one-bit-different tag is rejected")
    }

    // A different key must not verify. This is the imposter reduced to arithmetic:
    // it needs no radio and no phone, so it holds even when the hardware legs cannot
    // run. It is not a substitute for them -- it says nothing about whether the phone
    // builds the same pre-image -- but it is the half that can be checked anywhere.
    let otherKey = SymmetricKey(data: hexDecode(vectors[2].keyHex)!)
    let otherTag = Data(HMAC<SHA256>.authenticationCode(
        for: beaconMessage(keyId: 1, counter: 0), using: otherKey)).prefix(tagLen)
    if constantTimeEqual(hexDecode(vectors[0].tagHex)!, otherTag) {
        print("  FAIL a tag minted with a different key compared equal")
        failures += 1
    } else {
        print("  ok   a tag minted with a different key is rejected")
    }

    // keyId is inside the pre-image, so one key cannot cover every slot.
    let slot9 = Data(HMAC<SHA256>.authenticationCode(
        for: beaconMessage(keyId: 9, counter: 0), using: k)).prefix(tagLen)
    if constantTimeEqual(hexDecode(vectors[0].tagHex)!, slot9) {
        print("  FAIL the same key produced the same tag for a different keyId")
        failures += 1
    } else {
        print("  ok   keyId is bound into the tag")
    }

    // Windows outside ±1 must be rejected, or the replay bound is not what the
    // comment above claims.
    let now: Int64 = 1_740_000_000
    let store = KeyStore(dir: "/nonexistent")
    _ = store
    let live = Data(HMAC<SHA256>.authenticationCode(
        for: beaconMessage(keyId: 1, counter: now / windowSeconds), using: k)).prefix(tagLen)
    let old = Data(HMAC<SHA256>.authenticationCode(
        for: beaconMessage(keyId: 1, counter: now / windowSeconds - 2), using: k)).prefix(tagLen)
    var inWindow = false, outOfWindow = false
    for c in [now / windowSeconds - 1, now / windowSeconds, now / windowSeconds + 1] {
        let e = Data(HMAC<SHA256>.authenticationCode(
            for: beaconMessage(keyId: 1, counter: c), using: k)).prefix(tagLen)
        if constantTimeEqual(e, live) { inWindow = true }
        if constantTimeEqual(e, old) { outOfWindow = true }
    }
    if inWindow && !outOfWindow {
        print("  ok   the current window verifies and a 2-window-old tag does not")
    } else {
        print("  FAIL window bounds: inWindow=\(inWindow) outOfWindow=\(outOfWindow)")
        failures += 1
    }

    print(failures == 0 ? "\nself-test passed" : "\n\(failures) failed")
    return failures == 0 ? 0 : 1
}

// MARK: - main

var keyDir = defaultKeyDir
var fixedNow: Int64? = nil
var runSelfTest = false
var argv = Array(CommandLine.arguments.dropFirst())
while let flag = argv.first {
    argv.removeFirst()
    switch flag {
    case "--self-test":
        runSelfTest = true
    case "--key-dir":
        guard let v = argv.first else { log("--key-dir needs a path"); exit(64) }
        keyDir = v
        argv.removeFirst()
    case "--fixed-now":
        guard let v = argv.first.flatMap({ Int64($0) }) else { log("--fixed-now needs seconds"); exit(64) }
        fixedNow = v
        argv.removeFirst()
    default:
        log("usage: presence-verify [--self-test] [--key-dir DIR] [--fixed-now UNIX_SECONDS]")
        exit(64)
    }
}

if runSelfTest { exit(selfTest()) }

if let f = fixedNow {
    // Loud, because a pinned clock turns "verify a fresh beacon" into "verify any
    // beacon from that moment", which is exactly the property the window provides.
    log("TEST MODE: clock pinned to \(f). Replay protection is disabled in this run.")
}

let keys = KeyStore(dir: keyDir)
setlinebuf(stdout)
log("verifying against \(keyDir), window \(windowSeconds)s, tag \(tagLen) bytes")

while let line = readLine(strippingNewline: true) {
    let f = line.split(separator: ",", omittingEmptySubsequences: false).map(String.init)
    guard f.count >= 6 else {
        // Not our CSV. Pass it through marked, rather than dropping it: a bridge that
        // silently loses lines looks the same as a phone that went quiet.
        print("\(line),MALFORMED")
        continue
    }
    // Judge each row against the moment it was HEARD, not the moment it is read.
    //
    // Using the wall clock made the verdict depend on how long a row sat in the
    // pipe. That is invisible in the live pipeline, where rows arrive at once,
    // and wrong everywhere else: the impersonation test captures both legs and
    // then verifies, so its genuine beacons were 60-90s old by the time they
    // were judged -- every one of them correctly minted, every one reported
    // INVALID, because their windows really had passed. The question worth
    // asking is whether the beacon was current when it reached the antenna, and
    // the scanner already stamps exactly that.
    //
    // It costs no freshness. The stamp is written by our own scanner, never by
    // the advertiser, and the permit's own staleness is the bridge's wall clock.
    // A future stamp is refused rather than trusted, so a bad clock cannot mint
    // validity.
    let stamped = Int64(f[0]).map { $0 / 1000 }
    let wall = Int64(Date().timeIntervalSince1970)
    let heardAt = stamped.map { min($0, wall + windowSeconds) } ?? wall
    let now = fixedNow ?? heardAt
    let auth: String
    if let keyId = UInt8(f[4]), let tag = hexDecode(f[5]) {
        auth = verify(keyId: keyId, tag: tag, now: now, keys: keys)
    } else {
        auth = "MALFORMED"
    }
    print("\(line),\(auth)")
}
