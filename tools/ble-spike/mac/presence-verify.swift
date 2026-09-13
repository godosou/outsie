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
//       msg = "repose-presence-v2 beacon" ‖ key_id(1) ‖ c(8 BE) ‖ cmd(1) ‖ seq(4 BE)
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
import IOKit

let beaconLabel = "repose-presence-v2 beacon"

/// Domain separation for the Mac's OWN beacon -- the one that tells the phone
/// whether this Mac is locked. A different label from the phone's, because the
/// two directions must never be interchangeable: a tag minted for "the Mac is
/// unlocked" must not also verify as "the phone is present", or a recording of
/// one becomes a forgery of the other.
let macStateLabel = "repose-macstate-v2 beacon"

/// Two bytes naming THIS Mac, inside the authenticated message.
///
/// v1 carried only the key id, and one phone paired with several Macs holds one
/// key -- so every Mac's beacon looked identical to it. The phone could say "a
/// paired Mac near you is locked" and nothing about which, which is the kind of
/// answer that sends you to the wrong desk.
///
/// Derived, not assigned: SHA-256 of the hardware UUID, first two bytes. No
/// registry to keep in sync, and it survives a reinstall. Two bytes will
/// collide roughly once in 256 for someone with 25 Macs -- acceptable for a
/// label, which is why the name shown beside it comes from pairing and this is
/// only what ties the beacon to it.
func macIdentity() -> UInt16 {
    let port: mach_port_t
    if #available(macOS 12.0, *) { port = kIOMainPortDefault } else { port = kIOMasterPortDefault }
    let svc = IOServiceGetMatchingService(port, IOServiceMatching("IOPlatformExpertDevice"))
    defer { if svc != 0 { IOObjectRelease(svc) } }
    guard svc != 0,
          let cf = IORegistryEntryCreateCFProperty(svc, "IOPlatformUUID" as CFString, kCFAllocatorDefault, 0),
          let uuid = cf.takeRetainedValue() as? String
    else {
        // No identity is better than a made-up one: 0 is reserved and the
        // phone shows it as "分不清是哪一台" rather than as a Mac.
        return 0
    }
    let h = SHA256.hash(data: Data(uuid.utf8))
    let b = Array(h)
    return (UInt16(b[0]) << 8) | UInt16(b[1])
}

let macId: UInt16 = macIdentity()
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

func beaconMessage(keyId: UInt8, counter: Int64, cmd: UInt8 = 0, seq: UInt32 = 0) -> Data {
    var d = Data(beaconLabel.utf8)
    d.append(keyId)
    // Two's-complement big-endian, matching Kotlin's `ushr` on a signed Long. The
    // counter is a unix time divided by 30, so it will not be negative this century;
    // the bit pattern is what has to agree, and this is the same one.
    let u = UInt64(bitPattern: counter)
    for shift in stride(from: 56, through: 0, by: -8) {
        d.append(UInt8((u >> UInt64(shift)) & 0xFF))
    }
    // The command fields are INSIDE the tag. That is the whole reason a command
    // can be trusted: nothing on the air can add one, change 「锁屏」 into
    // 「允许解锁」, or edit the sequence number to dodge the replay check,
    // without the key.
    d.append(cmd)
    for shift in stride(from: 24, through: 0, by: -8) {
        d.append(UInt8((seq >> UInt32(shift)) & 0xFF))
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
    /// When each missing key was last looked for. See key(for:).
    private var lastMiss: [UInt8: TimeInterval] = [:]
    private let missRetrySeconds: TimeInterval = 1.0
    /// When each CACHED key's file was last confirmed to still exist. A found
    /// key used to be cached for the life of the process, so 「删掉这把钥匙」
    /// deleted the file and this verifier went on accepting the phone until
    /// the pipeline happened to restart. One stat a second per key is the
    /// price of a revocation that takes effect when it is made.
    private var lastSeen: [UInt8: TimeInterval] = [:]

    init(dir: String) { self.dir = dir }

    /// For the self-test only: keys supplied directly, no file, no root.
    ///
    /// The permission check in load() is a real defence and must not be
    /// bypassable from anywhere else — hence a separate initialiser rather than
    /// a flag threaded through the loading path.
    init(fixed: [UInt8: SymmetricKey]) {
        self.dir = ""
        for (id, k) in fixed { cache[id] = .key(k) }
    }

    /// A found key is cached forever; a MISSING one is re-checked.
    ///
    /// It used to cache both. That made pairing while the pipeline was already
    /// running a no-op that never recovered: the verifier had looked once,
    /// before the key existed, and kept that answer for the life of the
    /// process. The panel said 已配对 and 监测运行中, and nothing worked, and
    /// nothing would ever have started working.
    ///
    /// Re-checking is cheap but not free -- this runs once per advertisement,
    /// several times a second -- so a miss is retried at most once a second.
    /// That is well inside the time it takes a person to walk back to their
    /// Mac after pairing.
    func key(for keyId: UInt8) -> SymmetricKey? {
        let now = Date().timeIntervalSince1970
        if case .key(let k)? = cache[keyId] {
            // Still on disk? The self-test store (dir == "") has no files to
            // check. Otherwise, at most once a second, and only existence:
            // ownership and mode were checked when it was loaded, and a file
            // that changes under root is a different failure from a revocation.
            if dir.isEmpty { return k }
            if let seen = lastSeen[keyId], now - seen < missRetrySeconds { return k }
            if FileManager.default.fileExists(atPath: "\(dir)/presence-key.\(keyId)") {
                lastSeen[keyId] = now
                return k
            }
            cache[keyId] = nil
            lastSeen[keyId] = nil
            lastMiss[keyId] = now
            log("keyId \(keyId): key file gone, no longer accepted")
            return nil
        }

        if let last = lastMiss[keyId], now - last < missRetrySeconds {
            return nil
        }
        lastMiss[keyId] = now

        let loaded = load(keyId)
        switch loaded {
        case .key(let k):
            cache[keyId] = loaded
            lastMiss[keyId] = nil
            lastSeen[keyId] = now
            // Worth a line: it is the moment a Mac that was refusing everything
            // starts accepting the phone, and someone reading the log during a
            // pairing needs to see it happen.
            log("keyId \(keyId): key loaded")
            complained.remove(keyId)
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

func verify(keyId: UInt8, tag: Data, now: Int64, keys: KeyStore,
            cmd: UInt8 = 0, seq: UInt32 = 0) -> String {
    guard tag.count == tagLen else { return "MALFORMED" }
    guard let k = keys.key(for: keyId) else { return "NOKEY" }
    let c0 = now / windowSeconds
    for c in [c0 - 1, c0, c0 + 1] {
        let full = Data(HMAC<SHA256>.authenticationCode(
            for: beaconMessage(keyId: keyId, counter: c, cmd: cmd, seq: seq), using: k))
        if constantTimeEqual(full.prefix(tagLen), tag) { return "VALID" }
    }
    return "INVALID"
}

// MARK: - the Mac's own beacon

/// The pre-image for "this Mac is locked / unlocked".
///
/// Same shape as the phone's, different label and a state byte instead of a
/// command. The counter is still derived from each side's own clock and never
/// transmitted, so a recording ages out within a window either way.
func macStateMessage(keyId: UInt8, macId: UInt16, counter: Int64, state: UInt8) -> Data {
    var d = Data(macStateLabel.utf8)
    d.append(keyId)
    // Inside the pre-image, not merely alongside it: a mac id the phone reads
    // from an unauthenticated field is a label anyone can set, and a phone that
    // trusts it can be told the wrong Mac is locked.
    d.append(UInt8((macId >> 8) & 0xFF))
    d.append(UInt8(macId & 0xFF))
    let u = UInt64(bitPattern: counter)
    for shift in stride(from: 56, through: 0, by: -8) {
        d.append(UInt8((u >> UInt64(shift)) & 0xFF))
    }
    d.append(state)
    return d
}

/// Emit a tag for EVERY state, every window, on stdout.
///
/// WHY BOTH, AND WHY THIS PROCESS
///
/// Advertising needs the Bluetooth grant, which belongs to the app; the key is
/// root-only and must not leave this process. That is the same split that put
/// rssi-scan outside root in the first place. So the half with the key mints
/// the tags and the half with the radio picks one -- and minting both states
/// up front means the radio side never has to ask, which would be a round trip
/// on every lock and unlock.
///
/// Publishing the unlocked tag costs nothing: it says "this Mac is unlocked",
/// which anyone standing in front of it can already see.
func emitMacStateTags(keys: KeyStore, keyId: UInt8, now: Int64) {
    guard let k = keys.key(for: keyId) else { return }
    let c = now / windowSeconds
    // One tag per state the beacon may need to say: 0/1 for the lock state,
    // 2..7 for the phases of a calibration the phone asked for (design doc
    // §05, protocol §14). Minted here because only this process holds the
    // key; the beacon process picks one without ever seeing K.
    let tag = { (state: UInt8) -> String in
        Data(HMAC<SHA256>.authenticationCode(
            for: macStateMessage(keyId: keyId, macId: macId, counter: c, state: state), using: k))
            .prefix(tagLen).map { String(format: "%02x", $0) }.joined()
    }
    let tags = (0...7).map { tag(UInt8($0)) }.joined(separator: ",")
    print("macstate,\(keyId),\(c),\(tags),\(String(format: "%04x", macId))")
}

// MARK: - command replay defence

/// The highest command sequence accepted so far, per key.
///
/// A command rides in every advertisement for several seconds, so the same
/// (cmd, seq) is seen many times legitimately — the first sighting is the
/// command, the rest are echoes of it. Anyone with a radio can also record one
/// and re-send it later. Both are handled the same way: strictly-greater wins,
/// everything else is an echo.
///
/// Replaying a lock is harmless. Replaying 「允许解锁」 is not, and that is the
/// only reason this exists.
///
/// In memory, not on disk, and that is a real limitation worth naming: restart
/// the pipeline and the mark resets to zero, so a command recorded before the
/// restart would be accepted once. The window check still bounds that to about
/// a minute either side of when it was minted, so a recording is only useful to
/// someone who is standing there at the time — which is the same person who
/// could just watch you unlock. Persisting it is the fix if that stops being
/// good enough.
final class SeqGuard {
    private var high: [UInt8: UInt32] = [:]

    /// True if this command is new. Non-commands never consume a sequence.
    func accept(keyId: UInt8, cmd: UInt8, seq: UInt32) -> Bool {
        guard cmd != 0 else { return false }
        if let seen = high[keyId], seq <= seen { return false }
        high[keyId] = seq
        return true
    }
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
    let keyHex: String, keyId: UInt8, counter: Int64, cmd: UInt8, seq: UInt32, tagHex: String
}

/// The last two differ only in `cmd`. That pair is the assertion that the
/// command byte is really inside the pre-image: drop it and they collide, and
/// the self-test says so — rather than the phone's commands quietly becoming
/// editable by anyone with a radio.
let vectors: [Vector] = [
    Vector(keyHex: "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
           keyId: 1, counter: 0, cmd: 0, seq: 0, tagHex: "f5d57d0f0f6ae55b"),
    Vector(keyHex: "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
           keyId: 1, counter: 58000000, cmd: 0, seq: 0, tagHex: "1e53a75e337d1052"),
    Vector(keyHex: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
           keyId: 7, counter: 1, cmd: 0, seq: 0, tagHex: "c5fbe02ba6aa40b1"),
    Vector(keyHex: "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
           keyId: 1, counter: 58000000, cmd: 1, seq: 42, tagHex: "e51fae486a2159d0"),
    Vector(keyHex: "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
           keyId: 1, counter: 58000000, cmd: 2, seq: 42, tagHex: "e9b003496a5b1c05"),
]

/// Known answers for the Mac's own beacon, so the two implementations of
/// macStateMessage -- this one and MacStateScanner.kt -- cannot drift apart
/// without a test going red on one side.
///
/// Computed independently by macstate-vectors.py (standard-library hmac) before
/// being written down, not copied out of this binary's own output: a vector
/// produced by the code it checks agrees with itself no matter what either of
/// them says. A drift here reads on the phone as "the Mac stopped answering",
/// which is indistinguishable from being out of range and is the most
/// expensive kind of silent failure this product has.
///
/// state: 0 open · 1 locked · 2 measuring near · 3 near done, walk away ·
///        4 measuring far · 5 done · 6 both ends too alike · 7 no phone heard
struct MacStateVector {
    let keyHex: String
    let keyId: UInt8
    let macId: UInt16
    let counter: Int64
    let state: UInt8
    let tagHex: String
}

let macStateVectors: [MacStateVector] = [
    MacStateVector(keyHex: "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
                   keyId: 1, macId: 0xABCD, counter: 58000000, state: 1, tagHex: "d16d729bc487b663"),
    MacStateVector(keyHex: "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
                   keyId: 1, macId: 0xABCD, counter: 58000000, state: 0, tagHex: "ee8a24179231a934"),
    // A different Mac, everything else identical: the tag must change, or the
    // mac id is decoration rather than part of the authenticated message.
    MacStateVector(keyHex: "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
                   keyId: 1, macId: 0x0001, counter: 58000000, state: 1, tagHex: "eedf1ad32139ab89"),
    // The calibration states, 2..7. Each one is a distinct byte inside the
    // pre-image, so each gets its own tag: a phone that sees「等你走开」must
    // not be able to mistake it for「量好了」.
    MacStateVector(keyHex: "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
                   keyId: 1, macId: 0xABCD, counter: 58000000, state: 2, tagHex: "253ba303f4d5811e"),
    MacStateVector(keyHex: "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
                   keyId: 1, macId: 0xABCD, counter: 58000000, state: 3, tagHex: "7d66f7140359f142"),
    MacStateVector(keyHex: "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
                   keyId: 1, macId: 0xABCD, counter: 58000000, state: 4, tagHex: "cc9627b76c4d5dc3"),
    MacStateVector(keyHex: "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
                   keyId: 1, macId: 0xABCD, counter: 58000000, state: 5, tagHex: "557255691807c2ca"),
    MacStateVector(keyHex: "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
                   keyId: 1, macId: 0xABCD, counter: 58000000, state: 6, tagHex: "836f0d745518bc51"),
    MacStateVector(keyHex: "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
                   keyId: 1, macId: 0xABCD, counter: 58000000, state: 7, tagHex: "1f342de7dc9f5027"),
]

func selfTest() -> Int32 {
    var failures = 0

    for (i, v) in macStateVectors.enumerated() {
        let k = SymmetricKey(data: hexDecode(v.keyHex)!)
        let got = Data(HMAC<SHA256>.authenticationCode(
            for: macStateMessage(keyId: v.keyId, macId: v.macId, counter: v.counter, state: v.state),
            using: k)).prefix(tagLen)
        let gotHex = got.map { String(format: "%02x", $0) }.joined()
        if gotHex == v.tagHex {
            print("  ok   macstate vector \(i): macId=\(String(format: "%04x", v.macId)) state=\(v.state) -> \(gotHex)")
        } else {
            print("  FAIL macstate vector \(i): expected \(v.tagHex), got \(gotHex)")
            failures += 1
        }
    }
    for (i, v) in vectors.enumerated() {
        let k = SymmetricKey(data: hexDecode(v.keyHex)!)
        let got = Data(HMAC<SHA256>.authenticationCode(
            for: beaconMessage(keyId: v.keyId, counter: v.counter, cmd: v.cmd, seq: v.seq),
            using: k)).prefix(tagLen)
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

    // ---- the command fields are bound into the tag ----
    //
    // The vectors above already prove cmd is in the pre-image. These prove the
    // verifier REJECTS a mismatch, which is the direction that matters: a
    // verifier that computed the right tag but compared the wrong one would
    // pass every vector and still accept 「允许解锁」 from a packet that said
    // 「锁屏」.
    do {
        let k = SymmetricKey(data: hexDecode(vectors[3].keyHex)!)
        let store = KeyStore(fixed: [1: k])
        let now = vectors[3].counter * windowSeconds
        let tag = hexDecode(vectors[3].tagHex)!

        let right = verify(keyId: 1, tag: tag, now: now, keys: store, cmd: 1, seq: 42)
        let wrongCmd = verify(keyId: 1, tag: tag, now: now, keys: store, cmd: 2, seq: 42)
        let wrongSeq = verify(keyId: 1, tag: tag, now: now, keys: store, cmd: 1, seq: 43)
        if right == "VALID", wrongCmd == "INVALID", wrongSeq == "INVALID" {
            print("  ok   cmd and seq are covered by the tag")
        } else {
            print("  FAIL cmd/seq binding: right=\(right) wrongCmd=\(wrongCmd) wrongSeq=\(wrongSeq)")
            failures += 1
        }
    }

    // ---- replays are refused ----
    //
    // A command rides in every advertisement for several seconds, so the honest
    // majority of repeat sightings are echoes, not attacks. Both are refused the
    // same way, and that is the point: there is no way to tell them apart, so
    // "seen this sequence already" has to be the whole answer.
    do {
        let g = SeqGuard()
        let firstSighting = g.accept(keyId: 1, cmd: 2, seq: 10)
        let echo = g.accept(keyId: 1, cmd: 2, seq: 10)
        let older = g.accept(keyId: 1, cmd: 2, seq: 9)
        let newer = g.accept(keyId: 1, cmd: 2, seq: 11)
        let otherKeyUnaffected = g.accept(keyId: 2, cmd: 2, seq: 1)
        let notACommand = g.accept(keyId: 1, cmd: 0, seq: 99)
        if firstSighting, !echo, !older, newer, otherKeyUnaffected, !notACommand {
            print("  ok   a replayed command sequence is refused")
        } else {
            print("  FAIL replay guard: first=\(firstSighting) echo=\(echo) older=\(older) "
                  + "newer=\(newer) other=\(otherKeyUnaffected) noCmd=\(notACommand)")
            failures += 1
        }
    }

    print(failures == 0 ? "\nself-test passed" : "\n\(failures) failed")
    return failures == 0 ? 0 : 1
}

// MARK: - main

var keyDir = defaultKeyDir
var fixedNow: Int64? = nil
/// A file that exists for as long as this run should last. See the watcher.
var runFlag: String? = nil
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
    case "--run-flag":
        guard let v = argv.first else { log("--run-flag needs a path"); exit(64) }
        runFlag = v
        argv.removeFirst()
    case "--fixed-now":
        guard let v = argv.first.flatMap({ Int64($0) }) else { log("--fixed-now needs seconds"); exit(64) }
        fixedNow = v
        argv.removeFirst()
    default:
        log("usage: presence-verify [--self-test] [--key-dir DIR] [--run-flag PATH] [--fixed-now UNIX_SECONDS]")
        exit(64)
    }
}

if runSelfTest { exit(selfTest()) }

if let f = fixedNow {
    // Loud, because a pinned clock turns "verify a fresh beacon" into "verify any
    // beacon from that moment", which is exactly the property the window provides.
    log("TEST MODE: clock pinned to \(f). Replay protection is disabled in this run.")
}

// Stop when the run is over.
//
// This watched getppid() and exited when it changed. That is right for a direct
// child and WRONG here: in `tail | presence-verify | tee | bridge &` the shell
// forks a subshell that starts the members and then goes away, so the parent
// legitimately changes within milliseconds of starting. The guard fired on
// every healthy run -- the bridge logged "parent went away" and exited seconds
// after coming up, which took the permit with it. Presence stopped working and
// the lock command had nothing left to reach.
//
// The run flag has no such ambiguity: it is a file the unprivileged half
// creates before starting anything and removes when the run ends, and it is
// already what the privileged script watches. Same fact, one source.
//
// EOF on stdin is still the ordinary exit. This covers the case the parent
// check was aimed at: the supervisor dying without reaping us, leaving a root
// process nobody can signal.
if let flag = runFlag {
    DispatchQueue.global().async {
        while true {
            Thread.sleep(forTimeInterval: 2)
            if !FileManager.default.fileExists(atPath: flag) {
                log("run flag gone, exiting rather than verifying for nobody")
                exit(0)
            }
        }
    }
}

let keys = KeyStore(dir: keyDir)
let seqGuard = SeqGuard()
/// Which (window, keyId) pairs have already had their state tags published.
///
/// Per KEY, not just per window. With two phones paired, emitting once a window
/// for whichever key happened to arrive first meant each phone heard the Mac's
/// state every other window -- 60 seconds apart, against a 20-second staleness
/// limit on the phone. Both would have sat on 「不知道」 almost all the time.
var statePublished: Set<String> = []
var lastStateWindow: Int64 = .min
setlinebuf(stdout)
log("verifying against \(keyDir), window \(windowSeconds)s, tag \(tagLen) bytes")

while let line = readLine(strippingNewline: true) {
    let f = line.split(separator: ",", omittingEmptySubsequences: false).map(String.init)
    guard f.count >= 6 else {
        // Not our CSV. Pass it through marked, rather than dropping it: a bridge that
        // silently loses lines looks the same as a phone that went quiet.
        print("\(line),auth=MALFORMED,cmd=0")
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
    // cmd/seq are optional: a row from an older scanner has six fields and is
    // still a perfectly good presence reading.
    let cmd = f.count > 6 ? (UInt8(f[6]) ?? 0) : 0
    let seq = f.count > 7 ? (UInt32(f[7]) ?? 0) : 0

    let auth: String
    if let keyId = UInt8(f[4]), let tag = hexDecode(f[5]) {
        auth = verify(keyId: keyId, tag: tag, now: now, keys: keys, cmd: cmd, seq: seq)
    } else {
        auth = "MALFORMED"
    }

    // A command is only passed on if it verified AND is not an echo. Anything
    // else goes downstream as cmd=0, so the bridge never has to ask whether a
    // command it was handed is real -- there is exactly one place that decides.
    var emitCmd: UInt8 = 0
    if auth == "VALID", let keyId = UInt8(f[4]),
       seqGuard.accept(keyId: keyId, cmd: cmd, seq: seq) {
        emitCmd = cmd
        log("command \(cmd) seq=\(seq) accepted from keyId=\(keyId)")
    }

    // Self-describing, not positional.
    //
    // The verdict used to be "the seventh comma-separated field", and widening
    // the scanner's CSV by two columns silently moved it -- every row then read
    // as unverified, which is the safe direction but is still a whole feature
    // quietly not working. The consumer now looks for a named field, so adding
    // columns upstream cannot move it again.
    print("\(line),auth=\(auth),cmd=\(emitCmd)")

    // One macstate line per window, alongside the stream we are already in.
    // No timer, no second process: rows arrive several times a second while the
    // phone is anywhere near, and when they stop there is nobody to tell.
    let window = now / windowSeconds
    if window != lastStateWindow {
        // A new window invalidates every previous publication at once, which is
        // also what keeps this set from growing.
        lastStateWindow = window
        statePublished.removeAll(keepingCapacity: true)
    }
    if let keyId = UInt8(f[4]) {
        let mark = "\(window):\(keyId)"
        if !statePublished.contains(mark) {
            statePublished.insert(mark)
            emitMacStateTags(keys: keys, keyId: keyId, now: now)
        }
    }
}
