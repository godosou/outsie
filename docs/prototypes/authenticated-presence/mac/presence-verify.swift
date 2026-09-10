// Repose authenticated-presence — standalone verifier + self-test (no radio).
//
// Two modes, both offline:
//   presence-verify --self-test
//       Reproduce the committed known-key/time/tag vector and the ±1-window and
//       forgery paths. Exits 0 on pass, 1 on fail. Runs without a Bluetooth radio.
//
//   presence-verify --key <hex32|@file> --key-id <n> --tag <hex> [--now <unix_s>]
//       Verify one observed tag against K over current ± adjacent windows, exactly
//       as rssi-scan does live. Prints VALID / INVALID; exits 0 / 1.
//
// Build: swiftc -O -o presence-verify presence-verify.swift PresenceVerify.swift
//
// Shares all crypto/parsing with the live scanner via PresenceVerify.swift, so
// the self-test exercises the same code path that gates the permit.

import Foundation

@main
struct PresenceVerifyCLI {
    static func main() {
        var args = Array(CommandLine.arguments.dropFirst())

        if args.first == "--self-test" {
            let (ok, detail) = PresenceVerify.selfTest()
            FileHandle.standardError.write((detail + "\n").data(using: .utf8)!)
            print(ok ? "SELF-TEST PASS" : "SELF-TEST FAIL")
            exit(ok ? 0 : 1)
        }

        // One-shot verify mode.
        var keyArg: String?
        var keyIdArg: UInt8?
        var tagArg: String?
        var nowArg: UInt64 = UInt64(Date().timeIntervalSince1970)

        func need(_ label: String) -> String {
            guard !args.isEmpty else { die("missing value for \(label)") }
            return args.removeFirst()
        }
        while let flag = args.first {
            args.removeFirst()
            switch flag {
            case "--key":     keyArg = need("--key")
            case "--key-id":  keyIdArg = UInt8(need("--key-id"))
            case "--tag":     tagArg = need("--tag")
            case "--now":     nowArg = UInt64(need("--now")) ?? nowArg
            default:          die("usage: presence-verify --self-test | "
                                  + "--key <hex32|@file> --key-id <n> --tag <hex> [--now <unix_s>]")
            }
        }

        guard let keyRaw = keyArg, let keyId = keyIdArg, let tagHex = tagArg else {
            die("verify mode needs --key, --key-id, and --tag")
        }
        guard let key = resolveKey(keyRaw), key.count == 32 else {
            die("--key must be 32 raw bytes (64 hex chars) or @path to such a file")
        }
        guard let tag = PresenceVerify.hexDecode(tagHex), tag.count >= 8, tag.count <= 32 else {
            die("--tag must be 8..32 bytes of hex")
        }

        let valid = PresenceVerify.verify(key: key, keyId: keyId, tag: tag, nowSeconds: nowArg)
        print(valid ? "VALID" : "INVALID")
        exit(valid ? 0 : 1)
    }

    /// `--key` accepts inline hex, or `@path` to a raw/hex key file.
    static func resolveKey(_ arg: String) -> Data? {
        if arg.hasPrefix("@") {
            let path = String(arg.dropFirst())
            guard let data = FileManager.default.contents(atPath: path) else { return nil }
            if data.count == 32 { return data }
            if let text = String(data: data, encoding: .utf8) {
                return PresenceVerify.hexDecode(text.trimmingCharacters(in: .whitespacesAndNewlines))
            }
            return nil
        }
        return PresenceVerify.hexDecode(arg)
    }

    static func die(_ msg: String) -> Never {
        FileHandle.standardError.write(("presence-verify: " + msg + "\n").data(using: .utf8)!)
        exit(64)
    }
}
