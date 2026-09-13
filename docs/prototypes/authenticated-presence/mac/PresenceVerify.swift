// Outsie authenticated-presence — shared verification library (Mac side).
//
// This file holds NO top-level code and NO CoreBluetooth dependency, so it
// compiles into both binaries:
//   - rssi-scan       (live scanner; reads the adv, verifies, emits CSV)
//   - presence-verify (standalone verifier + --self-test; no radio)
//
// It implements the §3.2 verification of docs/plans/2026-09-10-authenticated-
// presence-design.md: recompute the rotating beacon tag
//     tag = HMAC-SHA256(K, "repose-presence-v1 beacon" ‖ keyId(1) ‖ counter(8 BE))[0..TAG_LEN)
// over the current ± adjacent windows and constant-time compare the truncated tag.
//
// No invented crypto: HMAC-SHA256 comes from Apple CryptoKit.

import CryptoKit
import Foundation

enum PresenceVerify {
    // --- Protocol constants: MUST match Android SpikeContract.kt ---------------
    /// 16-bit service UUID the presence beacon rides in (Service Data – 16-bit).
    static let serviceUUID16 = "FFF0"
    /// Advertisement format/version byte.
    static let version: UInt8 = 0x01
    /// Rotation window in seconds. counter = floor(unix_time / WINDOW).
    static let windowSeconds: UInt64 = 30
    /// Default truncated-tag length in bytes (8 → 64-bit forgery resistance).
    static let defaultTagLen = 8
    /// Domain-separation label for the beacon transcript (exact ASCII).
    static let beaconLabel = "repose-presence-v1 beacon"

    // --- Beacon math -----------------------------------------------------------

    /// msg = label ‖ keyId(1) ‖ counter(8, big-endian)
    static func beaconMessage(keyId: UInt8, counter: UInt64) -> Data {
        var msg = Data(beaconLabel.utf8)
        msg.append(keyId)
        var be = counter.bigEndian
        withUnsafeBytes(of: &be) { msg.append(contentsOf: $0) }
        return msg
    }

    /// Full HMAC-SHA256(K, msg), then truncate to `tagLen` bytes.
    /// Truncating an HMAC is safe (unlike truncating an ECDSA signature).
    static func beaconTag(key: Data, keyId: UInt8, counter: UInt64, tagLen: Int) -> Data {
        let msg = beaconMessage(keyId: keyId, counter: counter)
        let mac = HMAC<SHA256>.authenticationCode(for: msg, using: SymmetricKey(data: key))
        return Data(mac).prefix(tagLen)
    }

    /// Constant-time equality over equal-length byte strings. Lengths are public
    /// (fixed TAG_LEN), so the length guard leaks nothing.
    static func constantTimeEquals(_ a: Data, _ b: Data) -> Bool {
        guard a.count == b.count else { return false }
        var diff: UInt8 = 0
        for (x, y) in zip(a, b) { diff |= x ^ y }
        return diff == 0
    }

    /// §3.2: verify `tag` against `key` over windows {c0-1, c0, c0+1}. The counter
    /// is never transmitted — the Mac recomputes it from its own clock. The ±1
    /// window tolerates clock skew and the advertising gap around an RPA rotation.
    static func verify(key: Data, keyId: UInt8, tag: Data,
                       nowSeconds: UInt64, window: UInt64 = windowSeconds) -> Bool {
        let tagLen = tag.count
        guard tagLen >= 8, tagLen <= 32, window > 0 else { return false }
        let c0 = nowSeconds / window
        // c0-1, c0, c0+1 — guard the low end so c0==0 doesn't underflow UInt64.
        var counters: [UInt64] = [c0, c0 + 1]
        if c0 > 0 { counters.append(c0 - 1) }
        for c in counters {
            let expected = beaconTag(key: key, keyId: keyId, counter: c, tagLen: tagLen)
            if constantTimeEquals(expected, tag) { return true }
        }
        return false
    }

    // --- Advertisement service-data parsing ------------------------------------

    enum ParseError: Error { case tooShort, badVersion }

    /// Parse the Service-Data-16 payload: byte0=version, byte1=keyId, rest=tag.
    /// Returns nil if malformed (caller emits auth=NONE, not INVALID).
    static func parseServiceData(_ data: Data) -> (keyId: UInt8, tag: Data)? {
        let bytes = [UInt8](data)
        // version(1) + keyId(1) + at least an 8-byte tag.
        guard bytes.count >= 2 + 8 else { return nil }
        guard bytes[0] == version else { return nil }
        let keyId = bytes[1]
        let tag = Data(bytes[2...])
        guard tag.count <= 32 else { return nil }
        return (keyId, tag)
    }

    // --- Hex helpers -----------------------------------------------------------

    static func hexEncode(_ data: Data) -> String {
        data.map { String(format: "%02x", $0) }.joined()
    }

    static func hexDecode(_ s: String) -> Data? {
        let clean = s.hasPrefix("0x") ? String(s.dropFirst(2)) : s
        guard clean.count % 2 == 0 else { return nil }
        var out = Data(capacity: clean.count / 2)
        var idx = clean.startIndex
        while idx < clean.endIndex {
            let next = clean.index(idx, offsetBy: 2)
            guard let b = UInt8(clean[idx..<next], radix: 16) else { return nil }
            out.append(b)
            idx = next
        }
        return out
    }

    // --- Key loading -----------------------------------------------------------

    /// Default root-only presence-key directory (§1.5). File name:
    /// `presence-key.<keyId as %02x>`, 0600, raw 32-byte K.
    static let defaultKeyDir = "/var/db/repose-unlock"

    static func keyFilePath(dir: String, keyId: UInt8) -> String {
        "\(dir)/presence-key.\(String(format: "%02x", keyId))"
    }

    /// Load K for a given keyId from the root-only key dir. Returns nil if absent
    /// (unknown keyId ⇒ no such pairing on this Mac ⇒ INVALID).
    static func loadKey(dir: String, keyId: UInt8) -> Data? {
        let path = keyFilePath(dir: dir, keyId: keyId)
        guard let data = FileManager.default.contents(atPath: path) else { return nil }
        // Accept a raw 32-byte key, or a hex string (trailing newline tolerated).
        if data.count == 32 { return data }
        if let text = String(data: data, encoding: .utf8),
           let decoded = hexDecode(text.trimmingCharacters(in: .whitespacesAndNewlines)),
           decoded.count == 32 {
            return decoded
        }
        return nil
    }

    // --- Committed self-test vector (mirrors codex protocol/fixtures/v1) --------
    // Public, deterministic, test-only values — NEVER a production key. The
    // expected tag was computed by an independent HMAC-SHA256 reference (Python
    // hmac), so a passing self-test proves the Swift math matches the standard.
    enum Vector {
        // K = bytes 00..1f
        static let keyHex = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f"
        static let keyId: UInt8 = 0x01
        static let timestamp: UInt64 = 1_700_000_000   // → counter 56666666
        static let expectedCounter: UInt64 = 56_666_666
        static let expectedTag8Hex = "b358a3906712625b"
        static let expectedTag16Hex = "b358a3906712625b3c973b9a8859b2d8"
    }

    /// Offline self-test: reproduce the committed vector and exercise the ±1
    /// window tolerance and the negative (forgery) path. No radio.
    static func selfTest() -> (ok: Bool, detail: String) {
        guard let key = hexDecode(Vector.keyHex) else {
            return (false, "vector key hex failed to decode")
        }
        var lines: [String] = []
        var ok = true
        func check(_ name: String, _ cond: Bool) {
            lines.append("  \(cond ? "ok  " : "FAIL") \(name)")
            if !cond { ok = false }
        }

        // 1. counter derivation.
        let counter = Vector.timestamp / windowSeconds
        check("counter = floor(ts/WINDOW) = \(counter)", counter == Vector.expectedCounter)

        // 2. 8-byte tag reproduces the independent reference.
        let tag8 = beaconTag(key: key, keyId: Vector.keyId, counter: counter, tagLen: 8)
        check("tag8 = \(hexEncode(tag8))", hexEncode(tag8) == Vector.expectedTag8Hex)

        // 3. 16-byte tag reproduces the reference (truncation is a prefix).
        let tag16 = beaconTag(key: key, keyId: Vector.keyId, counter: counter, tagLen: 16)
        check("tag16 = \(hexEncode(tag16))", hexEncode(tag16) == Vector.expectedTag16Hex)

        // 4. verify() accepts the exact-window tag.
        check("verify accepts current window",
              verify(key: key, keyId: Vector.keyId, tag: tag8, nowSeconds: Vector.timestamp))

        // 5. ±1 window tolerance: a tag minted one window earlier still verifies
        //    when the clock has advanced into the next window.
        let tagPrev = beaconTag(key: key, keyId: Vector.keyId, counter: counter, tagLen: 8)
        check("verify accepts tag from adjacent (previous) window",
              verify(key: key, keyId: Vector.keyId, tag: tagPrev,
                     nowSeconds: Vector.timestamp + windowSeconds))

        // 6. window+2 away is rejected (no over-wide tolerance).
        check("verify rejects a tag two windows stale",
              !verify(key: key, keyId: Vector.keyId, tag: tag8,
                      nowSeconds: Vector.timestamp + 3 * windowSeconds))

        // 7. wrong key rejected.
        var wrong = [UInt8](key); wrong[0] ^= 0xff
        check("verify rejects wrong key",
              !verify(key: Data(wrong), keyId: Vector.keyId, tag: tag8, nowSeconds: Vector.timestamp))

        // 8. flipped-bit tag rejected (forgery path).
        var forged = [UInt8](tag8); forged[0] ^= 0x01
        check("verify rejects a 1-bit-flipped tag",
              !verify(key: key, keyId: Vector.keyId, tag: Data(forged), nowSeconds: Vector.timestamp))

        // 9. wrong keyId rejected (keyId is bound into the transcript).
        check("verify rejects wrong keyId",
              !verify(key: key, keyId: Vector.keyId ^ 0x01, tag: tag8, nowSeconds: Vector.timestamp))

        // 10. parse round-trip: build a payload, parse it back.
        var payload = Data([version, Vector.keyId]); payload.append(tag8)
        if let parsed = parseServiceData(payload) {
            check("parse round-trip keyId+tag",
                  parsed.keyId == Vector.keyId && constantTimeEquals(parsed.tag, tag8))
        } else {
            check("parse round-trip keyId+tag", false)
        }

        // 11. parse rejects a truncated payload.
        check("parse rejects too-short payload",
              parseServiceData(Data([version, Vector.keyId, 0x00])) == nil)

        return (ok, lines.joined(separator: "\n"))
    }
}
