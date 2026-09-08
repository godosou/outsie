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
    private var connectAttempts = 0
    private var connectGeneration = 0
    private var gaveUpOnRead = false
    private let maxConnectAttempts = 3
    private let connectTimeout: TimeInterval = 10

    func start() {
        central = CBCentralManager(delegate: self, queue: nil)
    }

    private func beginScan() {
        central.scanForPeripherals(
            withServices: [serviceUUID],
            options: [CBCentralManagerScanOptionAllowDuplicatesKey: true])
        log("SCANNING for service \(serviceUUID.uuidString)")
    }

    /// Every path that ends a connection attempt must come back here. Scanning is
    /// stopped while connecting, so any path that forgets to resume leaves the run
    /// producing no samples at all -- indistinguishable in the CSV from the phone
    /// having gone silent, which is the exact conclusion this tool exists to measure.
    private func resumeScanning() {
        connecting = nil
        beginScan()
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
        guard connectAttempts < maxConnectAttempts else {
            if !gaveUpOnRead {
                gaveUpOnRead = true
                log("GIVING UP on the hello read after \(maxConnectAttempts) attempts — "
                    + "continuing passive RSSI logging, which is what the run needs")
            }
            return
        }

        connectAttempts += 1
        connectGeneration += 1
        let generation = connectGeneration
        connecting = p
        p.delegate = self
        c.stopScan()
        log("CONNECTING to \(idPrefix) (rssi \(RSSI.intValue)) "
            + "attempt \(connectAttempts)/\(maxConnectAttempts)")
        c.connect(p, options: nil)

        // CoreBluetooth's connect() has no timeout of its own and scanning is stopped
        // here, so a connection that never completes would silently end the run.
        DispatchQueue.main.asyncAfter(deadline: .now() + connectTimeout) { [weak self] in
            guard let self, self.connectGeneration == generation, self.connecting != nil
            else { return }
            log("CONNECT TIMED OUT after \(Int(self.connectTimeout))s — resuming scan")
            self.central.cancelPeripheralConnection(p)
            self.resumeScanning()
        }
    }

    func centralManager(_ c: CBCentralManager, didConnect p: CBPeripheral) {
        log("CONNECTED, discovering services")
        p.discoverServices([serviceUUID])
    }

    func centralManager(_ c: CBCentralManager, didFailToConnect p: CBPeripheral,
                        error: Error?) {
        log("CONNECT FAILED: \(error?.localizedDescription ?? "unknown")")
        resumeScanning()
    }

    func centralManager(_ c: CBCentralManager, didDisconnectPeripheral p: CBPeripheral,
                        error: Error?) {
        log("DISCONNECTED: \(error?.localizedDescription ?? "clean")")
        resumeScanning()
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
