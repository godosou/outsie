// repose-pair-v2 — the Mac's side, over BLE.
//
// Scans for the phone's pairing service, connects, runs M1/P1/M2/P2, checks the
// commitment, and prints six digits for a human to compare with the phone's
// screen. Prints the derived key on stdout ONLY after the human types y.
//
//   ./pair-with-phone [--timeout SECONDS]
//
// stdout: on success, one line of 64 hex characters -- the key. Nothing else
//         goes to stdout, so a caller can pipe it straight into a file.
// stderr: everything a person reads.
//
// WHAT THIS TOOL DELIBERATELY DOES NOT DO
//
// It does not write the key anywhere. Writing it needs root, and a tool that
// both talks to strangers over a radio and holds root is a worse shape than two
// tools. The caller (provision-from-pairing.sh, or the app) takes the key on
// stdout and puts it where it belongs.
//
// Build: swiftc -O -o pair-with-phone pair-with-phone.swift -framework CoreBluetooth

import CoreBluetooth
import CryptoKit
import Foundation

let pairingService = CBUUID(string: "FFF1")
let charPKM = CBUUID(string: "FFF2")
let charPKP = CBUUID(string: "FFF3")
let charNM = CBUUID(string: "FFF4")
let charNP = CBUUID(string: "FFF5")
/// Display names, exchanged after the nonces and BEFORE the digits are shown.
/// Cosmetic: nothing here is covered by the SAS transcript, so a name proves
/// nothing about who is on the other end. See the note at exchangeNames().
let charName = CBUUID(string: "FFF6")
/// Who the phone is and which slot it wants: `keyId(1) ‖ phoneId(8)`.
///
/// Both go into the SAS transcript. Outside it, a man in the middle could
/// rewrite the slot number and have this Mac overwrite a DIFFERENT phone's key
/// without the six digits changing -- the one check a person actually performs.
let charIdentity = CBUUID(string: "FFF8")

let COMMIT_LABEL = "repose-pair-v3 commit"
let SAS_LABEL = "repose-pair-v3 sas"
let KDF_LABEL = "repose-pair-v3 presence-key"

/// A SECOND key from the same exchange, for signing the shortcut catalogue.
///
/// The presence key is root-only by design, so the app cannot sign with it --
/// and a catalogue the phone cannot check is a catalogue that can mislabel every
/// button on it. Same transcript, different HKDF info, so this key can sign a
/// catalogue and cannot mint a presence beacon. It lives in the app's data
/// directory: whoever steals it can forge a button list and still cannot open
/// this Mac.
let CONSOLE_KDF_LABEL = "repose-pair-v3 console-key"

/// Where to drop the six digits for a GUI caller. nil when a person is reading
/// stderr instead. See the write site for why this is not stderr scraping.
var digitsFile: String? = nil

/// Where to drop the phone's self-reported name for a GUI caller.
var peerFile: String? = nil

/// What to call this Mac on the phone's screen. The user's computer name.
func macDisplayName() -> String {
    let host = Host.current().localizedName ?? ProcessInfo.processInfo.hostName
    return String(host.prefix(60))
}

func say(_ s: String) { FileHandle.standardError.write("\(s)\n".data(using: .utf8)!) }

/// Write via a temp file and rename, so a reader polling the path never sees a
/// half-written value -- three digits taken for six would be worse than none.
func atomicWrite(_ text: String, to path: String) {
    let tmp = path + ".partial"
    try? Data(text.utf8).write(to: URL(fileURLWithPath: tmp))
    _ = try? FileManager.default.replaceItemAt(
        URL(fileURLWithPath: path), withItemAt: URL(fileURLWithPath: tmp))
    if FileManager.default.fileExists(atPath: tmp) {
        try? FileManager.default.moveItem(atPath: tmp, toPath: path)
    }
}
func hex(_ d: Data) -> String { d.map { String(format: "%02x", $0) }.joined() }
func ascii(_ s: String) -> Data { Data(s.utf8) }

// ---- the protocol's arithmetic, in one place -------------------------------
//
// These were duplicated: once here, inline in the delegate, and once in a
// separate pair-crypto.swift that the test suite exercised. So the tested code
// was not the running code -- the same shape as a staleness timer that worked
// under bash and never ran under sh. One implementation now, and --self-test
// points at it.

func commitment(pkM: Data, pkP: Data, np: Data) -> Data {
    Data(SHA256.hash(data: ascii(COMMIT_LABEL) + pkM + pkP + np))
}

func sasHash(pkM: Data, pkP: Data, nm: Data, np: Data, keyId: UInt8, phoneId: Data) -> Data {
    Data(SHA256.hash(data: ascii(SAS_LABEL) + pkM + pkP + nm + np + Data([keyId]) + phoneId))
}

/// Six digits, zero padded. Short on purpose: a person has to read it off two
/// screens and compare, and a longer string gets compared less carefully.
func sasDigits(_ hash: Data) -> String {
    let n = hash.prefix(4).reduce(UInt32(0)) { ($0 << 8) | UInt32($1) }
    return String(format: "%06d", n % 1_000_000)
}

/// HKDF-SHA256, salted with the transcript hash so that even a digit collision
/// leaves the two halves of a MITM holding different keys.
func deriveKey(ecdhX: Data, salt: Data, label: String = KDF_LABEL) -> Data {
    let prk = HMAC<SHA256>.authenticationCode(for: ecdhX, using: SymmetricKey(data: salt))
    return Data(HMAC<SHA256>.authenticationCode(
        for: ascii(label) + Data([0x01]),
        using: SymmetricKey(data: Data(prk))))
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

func constantTimeEqual(_ a: Data, _ b: Data) -> Bool {
    guard a.count == b.count else { return false }
    var diff: UInt8 = 0
    for (x, y) in zip(a, b) { diff |= x ^ y }
    return diff == 0
}

final class Pairer: NSObject, CBCentralManagerDelegate, CBPeripheralDelegate {
    private var central: CBCentralManager!
    private var phone: CBPeripheral?
    private var chars: [CBUUID: CBCharacteristic] = [:]

    private let sk = P256.KeyAgreement.PrivateKey()
    private var pkM: Data { sk.publicKey.x963Representation }
    private let nm: Data = {
        var b = Data(count: 16)
        _ = b.withUnsafeMutableBytes { SecRandomCopyBytes(kSecRandomDefault, 16, $0.baseAddress!) }
        return b
    }()

    private var pkP: Data?
    // Named apart from the global commitment() so neither shadows the other.
    private var phoneCommitment: Data?

    /// Held between the nonce reveal and the name exchange completing.
    private var pendingTranscript: Data?
    /// What the phone calls itself. Cosmetic — see charName.
    private var peerName: String?
    /// Held between reading Np and reading the identity, because the transcript
    /// needs both and they arrive in two callbacks.
    private var pendingNp: Data?
    private var keyId: UInt8 = 0
    private var phoneId = Data()

    func start() { central = CBCentralManager(delegate: self, queue: nil) }

    func centralManagerDidUpdateState(_ c: CBCentralManager) {
        switch c.state {
        case .poweredOn:
            say("扫描配对中的手机…（在手机上点「开始配对」）")
            c.scanForPeripherals(withServices: [pairingService], options: nil)
        case .unauthorized:
            say("没有蓝牙权限：系统设置 → 隐私与安全性 → 蓝牙，允许这个终端应用")
            exit(2)
        case .poweredOff:
            say("蓝牙没有打开")
            exit(2)
        default:
            break
        }
    }

    func centralManager(_ c: CBCentralManager, didDiscover p: CBPeripheral,
                        advertisementData: [String: Any], rssi RSSI: NSNumber) {
        let name = (advertisementData[CBAdvertisementDataLocalNameKey] as? String) ?? p.name ?? "未命名设备"
        say("找到 \(name)（\(RSSI) dBm），连接中…")
        c.stopScan()
        phone = p
        p.delegate = self
        c.connect(p, options: nil)
    }

    func centralManager(_ c: CBCentralManager, didConnect p: CBPeripheral) {
        p.discoverServices([pairingService])
    }

    func centralManager(_ c: CBCentralManager, didFailToConnect p: CBPeripheral, error: Error?) {
        say("连接失败：\(error?.localizedDescription ?? "未知")")
        exit(3)
    }

    func centralManager(_ c: CBCentralManager, didDisconnectPeripheral p: CBPeripheral, error: Error?) {
        // A disconnect mid-exchange is not a retry opportunity: the phone throws
        // its ephemerals away with the window, so continuing would be comparing
        // digits from two different sessions.
        say("连接断开：\(error?.localizedDescription ?? "正常结束")")
        exit(3)
    }

    func peripheral(_ p: CBPeripheral, didDiscoverServices error: Error?) {
        guard let svc = p.services?.first(where: { $0.uuid == pairingService }) else {
            say("这台设备没有配对服务"); exit(3)
        }
        p.discoverCharacteristics([charPKM, charPKP, charNM, charNP, charName, charIdentity], for: svc)
    }

    func peripheral(_ p: CBPeripheral, didDiscoverCharacteristicsFor svc: CBService, error: Error?) {
        for ch in svc.characteristics ?? [] { chars[ch.uuid] = ch }
        // Four are the protocol; the name is optional so an older phone still
        // pairs. A cosmetic field must never be the reason a key exchange fails.
        guard chars[charPKP] != nil, chars[charNM] != nil, chars[charNP] != nil,
              let pkmChar = chars[charPKM] else {
            say("配对服务不完整（找到 \(chars.count) 个特征值）"); exit(3)
        }
        // Required, unlike the name. Without it this Mac does not know which
        // slot the key belongs in, and guessing slot 1 is how a second phone
        // would silently overwrite the first.
        guard chars[charIdentity] != nil else {
            say("这部手机上的 App 太旧了，它还不会说自己用哪个钥匙编号。先在手机上更新 Outsie。")
            exit(3)
        }
        // A 65-byte write has to fit. Truncation here would be silent and would
        // surface later as a curve-validation failure on the phone, which reads
        // like a protocol bug rather than an MTU one.
        let maxWrite = p.maximumWriteValueLength(for: .withResponse)
        if maxWrite < 65 {
            say("这条连接一次只能写 \(maxWrite) 字节，放不下 65 字节的公钥"); exit(3)
        }
        say("M1：发送 Mac 的临时公钥")
        p.writeValue(pkM, for: pkmChar, type: .withResponse)
    }

    func peripheral(_ p: CBPeripheral, didWriteValueFor ch: CBCharacteristic, error: Error?) {
        if let e = error {
            // The phone refuses out-of-order or malformed messages outright, so
            // a write error here is usually the protocol being violated, not the
            // radio failing.
            say("手机拒绝了这条消息：\(e.localizedDescription)"); exit(4)
        }
        if ch.uuid == charPKM {
            say("P1：读取手机的公钥和承诺")
            p.readValue(for: chars[charPKP]!)
        } else if ch.uuid == charNM {
            say("P2：读取手机公布的 nonce")
            p.readValue(for: chars[charNP]!)
        } else if ch.uuid == charName {
            p.readValue(for: ch)
        }
    }

    func peripheral(_ p: CBPeripheral, didUpdateValueFor ch: CBCharacteristic, error: Error?) {
        if let e = error { say("读取失败：\(e.localizedDescription)"); exit(4) }
        guard let value = ch.value else { say("读到空值"); exit(4) }

        if ch.uuid == charPKP {
            guard value.count == 97 else {
                say("P1 长度不对：\(value.count) 字节，应为 97"); exit(4)
            }
            let key = value.prefix(65)
            guard (try? P256.KeyAgreement.PublicKey(x963Representation: key)) != nil else {
                say("手机的公钥不在 P-256 曲线上，中止"); exit(4)
            }
            pkP = Data(key)
            phoneCommitment = Data(value.suffix(32))
            say("M2：发送 Mac 的 nonce")
            p.writeValue(nm, for: chars[charNM]!, type: .withResponse)

        } else if ch.uuid == charNP {
            guard value.count == 16, pkP != nil, phoneCommitment != nil else {
                say("P2 长度不对或状态错乱"); exit(4)
            }
            pendingNp = Data(value)
            say("P3：读取手机的编号")
            p.readValue(for: chars[charIdentity]!)

        } else if ch.uuid == charIdentity {
            guard value.count == 9 else {
                say("P3 长度不对：\(value.count) 字节，应为 9"); exit(4)
            }
            guard let pkP, let theirCommitment = phoneCommitment, let np = pendingNp else {
                say("状态错乱"); exit(4)
            }
            keyId = value[value.startIndex]
            phoneId = Data(value.suffix(8))
            guard keyId != 0 else {
                say("手机报了编号 0，那不是一个有效的钥匙位"); exit(4)
            }

            // The check the whole protocol rests on. If this fails, somebody
            // chose their nonce after seeing ours.
            let expected = commitment(pkM: pkM, pkP: pkP, np: np)
            guard constantTimeEqual(expected, theirCommitment) else {
                say("")
                say("承诺校验失败：手机公布的 nonce 和它先前的承诺对不上。")
                say("这正是中间人会留下的痕迹。已中止，不要重试这次会话。")
                exit(5)
            }

            let transcript = sasHash(pkM: pkM, pkP: pkP, nm: nm, np: np, keyId: keyId, phoneId: phoneId)
            pendingTranscript = transcript

            // Trade names before showing the digits, finish after.
            //
            // Before, because the readLine() below blocks the main queue and
            // nothing else can be delivered while it waits. After, in the sense
            // that matters: a name is not shown next to the digits, only once
            // the human has confirmed them. An attacker-chosen string reading
            // "我的手机" sitting beside the one thing the user is supposed to
            // check would be reassurance the protocol did not earn.
            //
            // Optional throughout. A phone without the characteristic still
            // pairs; it just has no name to show.
            if let nameChar = chars[charName] {
                p.writeValue(Data(macDisplayName().utf8), for: nameChar, type: .withResponse)
            } else {
                finish(transcript: transcript)
            }

        } else if ch.uuid == charName {
            peerName = String(data: value.prefix(120), encoding: .utf8)?
                .trimmingCharacters(in: .whitespacesAndNewlines)
            if let t = pendingTranscript { finish(transcript: t) }
        }
    }

    /// Show the digits, take the answer, derive the key.
    ///
    /// Split out of the delegate only because the name exchange has to complete
    /// first; the sequence itself is unchanged.
    private func finish(transcript: Data) {
        guard let pkP else { say("状态错乱"); exit(4) }
        let digits = sasDigits(transcript)

        // Hand the digits to a GUI caller, if there is one.
        //
        // The app cannot scrape them out of the prose below: that text is
        // written for a person and gets reworded, and a pairing UI that
        // silently shows the wrong six digits is the exact failure this
        // protocol exists to prevent. So the machine-readable copy is its
        // own file, written before the prompt, and the app waits for it.
        if let path = digitsFile { atomicWrite(digits, to: path) }

        say("")
        say("  手机上应当显示同样的六位数字：")
        say("")
        say("        \(digits)")
        say("")
        say("  一致就在手机上确认，然后在这里输入 y。")
        say("  不一致说明有人在中间，直接回车中止。")
        FileHandle.standardError.write("  一致吗？[y/N] ".data(using: .utf8)!)

        guard let reply = readLine(strippingNewline: true)?.lowercased(),
              reply == "y" || reply == "yes" else {
            say("已中止。这次的临时密钥作废。")
            exit(6)
        }

        // Only now. A name captured from a session the human rejected is the
        // attacker's name, and writing it would put it on the success screen.
        if let path = peerFile, let n = peerName, !n.isEmpty { atomicWrite(n, to: path) }

        do {
            let shared = try sk.sharedSecretFromKeyAgreement(
                with: P256.KeyAgreement.PublicKey(x963Representation: pkP))
            let x = shared.withUnsafeBytes { Data($0) }
            // stdout, alone, so the caller can redirect it into a file.
            //
            // Three fields now: the slot the phone asked for, who the phone
            // says it is, and the key. The caller needs all three -- the slot
            // decides where the key is installed and the identity decides which
            // older key, if any, this one replaces. Both were covered by the
            // digits the human just compared.
            print("\(keyId) \(hex(phoneId)) \(hex(deriveKey(ecdhX: x, salt: transcript))) \(hex(deriveKey(ecdhX: x, salt: transcript, label: CONSOLE_KDF_LABEL)))")
            say("配对成功。\(peerName.map { "对方设备：\($0)" } ?? "")")
            exit(0)
        } catch {
            say("派生密钥失败：\(error)"); exit(4)
        }
    }
}

// ---- self test -------------------------------------------------------------
//
// Vectors from tools/ble-spike/mac/pair-vectors.sh, which uses OpenSSL and
// shares no code with this file or with the phone. Each side is measured against
// a third opinion; checking the two implementations against each other would
// pass just as happily if both were wrong the same way.

enum Vec {
    static let skM = "b96e0676189db5cb28470e4dca48941385603d5e627726997fb97321746e69f9"
    static let pkM = "04ceeac1a625c3039e5e3176362e2b257461c66a8b3a5b23dbe67424c84b3f1202f34dd8c37e726e0474d28ff984c6fad4d3158baae761a1cfa50d3575d4d2af21"
    static let pkP = "04bba0ac866c040dee63395dc7ea9cd2ae65df7475c35295da07264de36a85dcc78f30b930a1af3147ac63a0d902593e53a78f7c9ab8773670562f50afea4bf035"
    static let nm = "000102030405060708090a0b0c0d0e0f"
    static let np = "f0e0d0c0b0a090807060504030201000"
    /// v3 folds the slot number and the phone's identity into the transcript,
    /// so every value below changed. Recomputed with Python's hashlib/hmac, not
    /// read out of this binary.
    static let keyId: UInt8 = 37
    static let phoneId = "0badc0de0badc0de"
    static let commit = "952be98c1c4aa1d54d4c416d56b72c333277976c70db3e6306af6c4f2ec91c6c"
    static let digits = "336549"
    static let ecdhX = "8264224d7eb11f8f5240fe1b94a14c7202a9c493cdfc5fd406f6a46d681f6f94"
    static let key = "1c58d63ba975bb90784b4bc442d79b6841ccf58c5a446b2a679f015502f4edcb"
    static let consoleKey = "ba6797f63dd6a57b39bcd0bb509e14481303d8ef505d35e27e7d21a8fd37181d"
}

func selfTest() -> Int32 {
    var f = 0
    func check(_ name: String, _ got: String, _ want: String) {
        if got == want { say("  ok   \(name)") }
        else { say("  FAIL \(name)\n       got  \(got)\n       want \(want)"); f += 1 }
    }
    let pkM = hexDecode(Vec.pkM)!, pkP = hexDecode(Vec.pkP)!
    let nm = hexDecode(Vec.nm)!, np = hexDecode(Vec.np)!

    check("commitment", hex(commitment(pkM: pkM, pkP: pkP, np: np)), Vec.commit)
    let t = sasHash(pkM: pkM, pkP: pkP, nm: nm, np: np, keyId: Vec.keyId, phoneId: hexDecode(Vec.phoneId)!)
    check("six digits", sasDigits(t), Vec.digits)

    do {
        let sk = try P256.KeyAgreement.PrivateKey(rawRepresentation: hexDecode(Vec.skM)!)
        let peer = try P256.KeyAgreement.PublicKey(x963Representation: pkP)
        let x = try sk.sharedSecretFromKeyAgreement(with: peer).withUnsafeBytes { Data($0) }
        check("ECDH x", hex(x), Vec.ecdhX)
        check("derived key", hex(deriveKey(ecdhX: x, salt: t)), Vec.key)
        check("console key", hex(deriveKey(ecdhX: x, salt: t, label: CONSOLE_KDF_LABEL)), Vec.consoleKey)
        // The separation is the whole point: one signs catalogues, the other
        // opens the Mac, and neither can stand in for the other.
        if hex(deriveKey(ecdhX: x, salt: t)) == hex(deriveKey(ecdhX: x, salt: t, label: CONSOLE_KDF_LABEL)) {
            say("  FAIL the console key is the presence key"); f += 1
        } else {
            say("  ok   the console key is a different key")
        }
    } catch {
        say("  FAIL ECDH threw: \(error)"); f += 1
    }

    // The check that stops a man in the middle, asserted directly rather than
    // implied by a happy path that happens to go in order.
    var tampered = np
    tampered[tampered.startIndex] ^= 0x01
    if constantTimeEqual(commitment(pkM: pkM, pkP: pkP, np: tampered), hexDecode(Vec.commit)!) {
        say("  FAIL a substituted nonce satisfied the commitment"); f += 1
    } else {
        say("  ok   a substituted nonce breaks the commitment")
    }

    // A swapped peer key must move the digits, or the SAS is not binding the
    // keys it exists to authenticate.
    var other = pkP
    other[other.startIndex + 1] ^= 0x01
    let pid = hexDecode(Vec.phoneId)!
    if sasDigits(sasHash(pkM: pkM, pkP: other, nm: nm, np: np, keyId: Vec.keyId, phoneId: pid)) == Vec.digits {
        say("  FAIL a substituted public key left the digits unchanged"); f += 1
    } else {
        say("  ok   a substituted public key changes the digits")
    }

    // The whole reason the slot number is in the transcript. A man in the middle
    // who could change it unnoticed would have this Mac install a key into
    // another phone's slot, replacing that phone's key with one he chose.
    if sasDigits(sasHash(pkM: pkM, pkP: pkP, nm: nm, np: np, keyId: Vec.keyId ^ 1, phoneId: pid)) == Vec.digits {
        say("  FAIL a rewritten key slot left the digits unchanged"); f += 1
    } else {
        say("  ok   a rewritten key slot changes the digits")
    }

    var otherPhone = pid
    otherPhone[otherPhone.startIndex] ^= 0x01
    if sasDigits(sasHash(pkM: pkM, pkP: pkP, nm: nm, np: np, keyId: Vec.keyId, phoneId: otherPhone)) == Vec.digits {
        say("  FAIL a rewritten phone identity left the digits unchanged"); f += 1
    } else {
        say("  ok   a rewritten phone identity changes the digits")
    }

    // Points off the curve are refused before any secret touches them.
    var offCurve = pkP
    offCurve[offCurve.endIndex - 1] ^= 0x01
    if (try? P256.KeyAgreement.PublicKey(x963Representation: offCurve)) != nil {
        say("  FAIL accepted a point off the curve"); f += 1
    } else {
        say("  ok   a point off the curve is refused")
    }

    say(f == 0 ? "\nself-test passed" : "\n\(f) failed")
    return f == 0 ? 0 : 1
}

var timeout: Double = 120
var argv = Array(CommandLine.arguments.dropFirst())
while let flag = argv.first {
    argv.removeFirst()
    if flag == "--self-test" {
        exit(selfTest())
    } else if flag == "--timeout", let v = argv.first.flatMap(Double.init) {
        timeout = v; argv.removeFirst()
    } else if flag == "--digits-file", let v = argv.first {
        digitsFile = v; argv.removeFirst()
    } else if flag == "--peer-file", let v = argv.first {
        peerFile = v; argv.removeFirst()
    } else {
        say("usage: pair-with-phone [--timeout SECONDS] [--digits-file PATH] [--peer-file PATH] | --self-test")
        exit(64)
    }
}

DispatchQueue.main.asyncAfter(deadline: .now() + timeout) {
    say("超时：没有在 \(Int(timeout)) 秒内完成配对")
    exit(7)
}

let pairer = Pairer()
pairer.start()
dispatchMain()
