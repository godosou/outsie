// send-catalogue — hand the phone the list of buttons it may show.
//
// Runs when the phone asks (cmd=3 in its beacon) and the user has the window
// open on their phone. Scans for the console service, connects, writes the
// catalogue in chunks, disconnects.
//
// WHY THE CATALOGUE IS SIGNED, AND WITH WHICH KEY
//
// A forged catalogue does not have to do anything clever: it mislabels the
// buttons. One reading 「锁屏」 sends a byte that means something else in this
// Mac's own configuration, and the person presses one thing believing it is
// another.
//
// So it carries an HMAC -- under the CONSOLE key from pairing, not the presence
// key. The presence key is root-only 0600 by design and this process runs as
// the user; the console key is derived from the same SAS under a different HKDF
// label, lives beside the pairing state at 0600, and can sign a button list
// while being useless for minting a presence beacon. Whoever steals it can
// forge a button list and still cannot open this Mac.
//
//   whole = version(1) ‖ revision(4 BE) ‖ jsonUtf8 ‖ tag(16)
//   tag   = HMAC(K_cat, "repose-console-v1 catalogue" ‖ version ‖ revision ‖ json)[0..16)
//   chunk = idx(2 BE) ‖ total(2 BE) ‖ bytes
//
// usage: send-catalogue --key <hex64> --revision <n> --json <path> [--timeout <s>]
// stdout: nothing. stderr: human-readable progress. exit 0 only if it landed.
//
// Build: swiftc -O -o send-catalogue send-catalogue.swift -framework CoreBluetooth

import CoreBluetooth
import CryptoKit
import Foundation

let consoleUUID = CBUUID(string: "FFF9")
let catalogueChar = CBUUID(string: "FFFA")
let catalogueVersion: UInt8 = 0x01
let tagLen = 16
let label = "repose-console-v1 catalogue"

func say(_ s: String) {
    FileHandle.standardError.write("send-catalogue: \(s)\n".data(using: .utf8)!)
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

// ---- arguments ------------------------------------------------------------

var keyHex = "", jsonPath = ""
var revision: UInt32 = 0
var timeout: TimeInterval = 45

var args = Array(CommandLine.arguments.dropFirst())
while let a = args.first {
    args.removeFirst()
    switch a {
    case "--key": keyHex = args.first ?? ""; if !args.isEmpty { args.removeFirst() }
    case "--json": jsonPath = args.first ?? ""; if !args.isEmpty { args.removeFirst() }
    case "--revision": revision = UInt32(args.first ?? "0") ?? 0; if !args.isEmpty { args.removeFirst() }
    case "--timeout": timeout = TimeInterval(args.first ?? "45") ?? 45; if !args.isEmpty { args.removeFirst() }
    default: say("unknown argument \(a)"); exit(2)
    }
}

guard let keyData = hexDecode(keyHex), keyData.count == 32 else {
    say("--key must be 64 hex characters")
    exit(2)
}
guard let json = FileManager.default.contents(atPath: jsonPath) else {
    say("cannot read \(jsonPath)")
    exit(2)
}

// ---- the payload ----------------------------------------------------------

var body = Data([catalogueVersion])
for shift in stride(from: 24, through: 0, by: -8) {
    body.append(UInt8((revision >> UInt32(shift)) & 0xFF))
}
body.append(json)

let tag = Data(HMAC<SHA256>.authenticationCode(
    for: Data(label.utf8) + body,
    using: SymmetricKey(data: keyData))).prefix(tagLen)
let whole = body + tag

// ---- the radio ------------------------------------------------------------

final class Sender: NSObject, CBCentralManagerDelegate, CBPeripheralDelegate {
    private var manager: CBCentralManager!
    private var peripheral: CBPeripheral?
    private var chunks: [Data] = []
    private var sent = 0

    func start() { manager = CBCentralManager(delegate: self, queue: nil) }

    func centralManagerDidUpdateState(_ c: CBCentralManager) {
        switch c.state {
        case .poweredOn:
            say("looking for the phone")
            c.scanForPeripherals(withServices: [consoleUUID])
        case .unauthorized:
            say("no Bluetooth permission for this process")
            exit(3)
        case .poweredOff:
            say("bluetooth is off")
            exit(3)
        default:
            break
        }
    }

    func centralManager(_ c: CBCentralManager, didDiscover p: CBPeripheral,
                        advertisementData: [String: Any], rssi: NSNumber) {
        c.stopScan()
        peripheral = p
        p.delegate = self
        say("found it, connecting")
        c.connect(p)
    }

    func centralManager(_ c: CBCentralManager, didConnect p: CBPeripheral) {
        p.discoverServices([consoleUUID])
    }

    func centralManager(_ c: CBCentralManager, didFailToConnect p: CBPeripheral, error: Error?) {
        say("could not connect: \(error?.localizedDescription ?? "no reason given")")
        exit(4)
    }

    func peripheral(_ p: CBPeripheral, didDiscoverServices error: Error?) {
        guard let svc = p.services?.first(where: { $0.uuid == consoleUUID }) else {
            say("the phone is not offering the console service"); exit(4)
        }
        p.discoverCharacteristics([catalogueChar], for: svc)
    }

    func peripheral(_ p: CBPeripheral, didDiscoverCharacteristicsFor svc: CBService, error: Error?) {
        guard let ch = svc.characteristics?.first(where: { $0.uuid == catalogueChar }) else {
            say("no catalogue characteristic"); exit(4)
        }
        // Four bytes of header per chunk, and stay inside what this link will
        // take in one write. Guessing high here does not fail loudly -- it
        // fails as a write the phone never sees.
        let room = max(20, p.maximumWriteValueLength(for: .withResponse)) - 4
        var parts: [Data] = []
        var at = whole.startIndex
        while at < whole.endIndex {
            let end = whole.index(at, offsetBy: room, limitedBy: whole.endIndex) ?? whole.endIndex
            parts.append(Data(whole[at..<end]))
            at = end
        }
        guard parts.count <= 512 else { say("catalogue too large"); exit(4) }

        chunks = parts.enumerated().map { (i, data) in
            var d = Data()
            d.append(UInt8((i >> 8) & 0xFF)); d.append(UInt8(i & 0xFF))
            d.append(UInt8((parts.count >> 8) & 0xFF)); d.append(UInt8(parts.count & 0xFF))
            d.append(data)
            return d
        }
        say("sending \(whole.count) bytes in \(chunks.count) chunks")
        write(p, ch)
    }

    private var characteristic: CBCharacteristic?

    private func write(_ p: CBPeripheral, _ ch: CBCharacteristic) {
        characteristic = ch
        guard sent < chunks.count else {
            say("done")
            exit(0)
        }
        p.writeValue(chunks[sent], for: ch, type: .withResponse)
    }

    func peripheral(_ p: CBPeripheral, didWriteValueFor ch: CBCharacteristic, error: Error?) {
        if let error {
            // The phone refuses a chunk it cannot place, so this is usually the
            // protocol disagreeing, not the radio failing.
            say("the phone refused chunk \(sent): \(error.localizedDescription)")
            exit(5)
        }
        sent += 1
        write(p, ch)
    }
}

let sender = Sender()
sender.start()

DispatchQueue.global().asyncAfter(deadline: .now() + timeout) {
    say("gave up after \(Int(timeout))s -- the phone was not offering to receive")
    exit(6)
}

dispatchMain()
