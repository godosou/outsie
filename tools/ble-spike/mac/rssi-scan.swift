// Repose BLE spike — Mac central.
// Scans for the fixed spike service, prints one CSV row per discovery, and
// reads the hello characteristic exactly once. No crypto, no state machine,
// no distance logic. Contract shared with tools/ble-spike/android.
//
// stdout: CSV only  ->  unix_ms,rssi,peripheral_id_prefix
// stderr: human-readable events (manager state, connect, read result)
//
// Build: swiftc -O -o rssi-scan rssi-scan.swift -framework CoreBluetooth
// Run:   ./rssi-scan [--duration SECONDS]

import CoreBluetooth
import Foundation

let serviceUUID = CBUUID(string: "7265706F-7365-0001-8000-00805F9B34FB")
let charUUID = CBUUID(string: "7265706F-7365-0002-8000-00805F9B34FB")

func log(_ msg: String) {
    let ts = ISO8601DateFormatter().string(from: Date())
    FileHandle.standardError.write("[\(ts)] \(msg)\n".data(using: .utf8)!)
}

final class Scanner: NSObject, CBCentralManagerDelegate, CBPeripheralDelegate {
    private var central: CBCentralManager!
    private var helloRead = false
    private var connecting: CBPeripheral?

    func start() {
        central = CBCentralManager(delegate: self, queue: nil)
    }

    private func beginScan() {
        central.scanForPeripherals(
            withServices: [serviceUUID],
            options: [CBCentralManagerScanOptionAllowDuplicatesKey: true])
        log("SCANNING for service \(serviceUUID.uuidString)")
    }

    func centralManagerDidUpdateState(_ c: CBCentralManager) {
        switch c.state {
        case .poweredOn:
            log("STATE poweredOn")
            beginScan()
        case .unauthorized:
            log("STATE unauthorized — grant Bluetooth to the parent terminal app in "
                + "System Settings > Privacy & Security > Bluetooth, then rerun")
            exit(2)
        case .poweredOff:
            log("STATE poweredOff — Bluetooth is off")
        case .unsupported:
            log("STATE unsupported — no BLE radio")
            exit(3)
        case .resetting:
            log("STATE resetting")
        default:
            log("STATE unknown(\(c.state.rawValue))")
        }
    }

    func centralManager(_ c: CBCentralManager, didDiscover p: CBPeripheral,
                        advertisementData: [String: Any], rssi RSSI: NSNumber) {
        let ms = Int(Date().timeIntervalSince1970 * 1000)
        let idPrefix = String(p.identifier.uuidString.prefix(8))
        print("\(ms),\(RSSI.intValue),\(idPrefix)")

        guard !helloRead, connecting == nil else { return }
        connecting = p
        p.delegate = self
        c.stopScan()
        log("CONNECTING to \(idPrefix) (rssi \(RSSI.intValue))")
        c.connect(p, options: nil)
    }

    func centralManager(_ c: CBCentralManager, didConnect p: CBPeripheral) {
        log("CONNECTED, discovering services")
        p.discoverServices([serviceUUID])
    }

    func centralManager(_ c: CBCentralManager, didFailToConnect p: CBPeripheral,
                        error: Error?) {
        log("CONNECT FAILED: \(error?.localizedDescription ?? "unknown")")
        connecting = nil
        beginScan()
    }

    func centralManager(_ c: CBCentralManager, didDisconnectPeripheral p: CBPeripheral,
                        error: Error?) {
        log("DISCONNECTED: \(error?.localizedDescription ?? "clean")")
        connecting = nil
        beginScan()
    }

    func peripheral(_ p: CBPeripheral, didDiscoverServices error: Error?) {
        if let e = error { log("SERVICE DISCOVERY FAILED: \(e.localizedDescription)") }
        guard let svc = p.services?.first(where: { $0.uuid == serviceUUID }) else {
            log("SERVICE \(serviceUUID.uuidString) NOT FOUND on peripheral")
            central.cancelPeripheralConnection(p)
            return
        }
        p.discoverCharacteristics([charUUID], for: svc)
    }

    func peripheral(_ p: CBPeripheral, didDiscoverCharacteristicsFor svc: CBService,
                    error: Error?) {
        if let e = error { log("CHAR DISCOVERY FAILED: \(e.localizedDescription)") }
        guard let ch = svc.characteristics?.first(where: { $0.uuid == charUUID }) else {
            log("CHARACTERISTIC \(charUUID.uuidString) NOT FOUND")
            central.cancelPeripheralConnection(p)
            return
        }
        p.readValue(for: ch)
    }

    func peripheral(_ p: CBPeripheral, didUpdateValueFor ch: CBCharacteristic,
                    error: Error?) {
        if let e = error {
            log("READ FAILED: \(e.localizedDescription)")
        } else {
            let bytes = ch.value ?? Data()
            let text = String(data: bytes, encoding: .utf8) ?? "<non-utf8>"
            let ok = text == "repose-hello" ? "MATCH" : "MISMATCH"
            log("READ \(ok): \"\(text)\" (\(bytes.count) bytes)")
            helloRead = true
        }
        central.cancelPeripheralConnection(p)
    }
}

setlinebuf(stdout)

var duration: Double? = nil
var args = Array(CommandLine.arguments.dropFirst())
while let flag = args.first {
    args.removeFirst()
    if flag == "--duration", let v = args.first.flatMap(Double.init) {
        duration = v
        args.removeFirst()
    } else {
        log("usage: rssi-scan [--duration SECONDS]")
        exit(64)
    }
}

if let d = duration {
    log("will exit after \(Int(d))s")
    DispatchQueue.main.asyncAfter(deadline: .now() + d) {
        log("duration elapsed, exiting")
        exit(0)
    }
}

let scanner = Scanner()
scanner.start()
dispatchMain()
