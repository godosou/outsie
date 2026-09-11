// Repose presence scanner — Mac central.
//
// Reads the `repose-presence-v1` beacon out of the advertisement and prints it.
// It decides NOTHING and holds NO key: verification is `presence-verify`, which
// runs as root with the paired key, and the near/far decision is `permit-bridge.sh`.
//
// That split is deliberate. This process needs the Bluetooth TCC grant and talks to
// the radio; the process holding the 256-bit presence key needs neither. Keeping the
// key out of the radio-facing process means a bug here cannot leak it.
//
// WHAT WAS DELETED, AND WHY
// -------------------------
// The B-spike connected to the peripheral, discovered a service, read a characteristic
// and compared it to the string "repose-hello". Both the UUID and the string are
// published in this repository, so that check identified nothing: any device
// rebroadcasting them passed (E13). It also cost a connect + discover + read on the
// unlock hot path, roughly 1-2s against a 1.5s permit budget. The connect path is gone;
// the authenticator now arrives inside the first advertisement, with no round trip.
//
// stdout: CSV only  ->  unix_ms,rssi,peer_prefix,ver,key_id,tag_hex,cmd,seq
//                       (every field after rssi is "-" when the packet carries no
//                        well-formed payload -- a seen-but-unusable device is a fact
//                        worth passing on, not a line to drop)
//
// cmd/seq are the phone->Mac command channel. They are passed through verbatim
// and UNVERIFIED: this process holds no key and decides nothing. Whether a
// command is authentic, and whether it is a replay, is presence-verify's call.
// stderr: human-readable events
//
// Build: swiftc -O -o rssi-scan rssi-scan.swift -framework CoreBluetooth
// Run:   ./rssi-scan [--duration SECONDS]

import CoreBluetooth
import Foundation

// 16-bit 0xFFF0. CoreBluetooth returns service-data keys in their short form, so the
// dictionary lookup must use the same short CBUUID rather than the 128-bit expansion.
let presenceUUID = CBUUID(string: "FFF0")

let presenceVersion: UInt8 = 0x02
let tagLen = 8
/// version(1) + keyId(1) + cmd(1) + seq(4) + tag(8)
let payloadLen = 7 + tagLen

/// Scan every advertiser instead of letting the kernel filter to ours. See
/// beginScan() for why: it is the only way to tell a quiet radio from a departed
/// phone.
var scanAll = false

func log(_ msg: String) {
    let ts = ISO8601DateFormatter().string(from: Date())
    FileHandle.standardError.write("[\(ts)] \(msg)\n".data(using: .utf8)!)
}

func hex(_ d: Data) -> String { d.map { String(format: "%02x", $0) }.joined() }

final class Scanner: NSObject, CBCentralManagerDelegate {
    private var central: CBCentralManager!
    private var sawPayload = false

    func start() { central = CBCentralManager(delegate: self, queue: nil) }

    private func beginScan() {
        // Filtered: the UUID-list AD is what `withServices:` matches; service data
        // alone does not satisfy it, which is why the beacon carries both.
        //
        // Unfiltered: see everything, and mark which rows are ours. Costs more
        // callbacks; buys the one thing a filtered scan can never tell you --
        // whether the radio is delivering at all. Without it, "the phone left"
        // and "CoreBluetooth went quiet" are the same observation, and the
        // staleness timeout has to be tuned against the larger of the two.
        central.scanForPeripherals(
            withServices: scanAll ? nil : [presenceUUID],
            options: [CBCentralManagerScanOptionAllowDuplicatesKey: true])
        log(scanAll ? "SCANNING all advertisers (ours: \(presenceUUID.uuidString))"
                    : "SCANNING for service \(presenceUUID.uuidString)")
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

        var ver = "-", keyId = "-", tag = "-", cmd = "-", seq = "-"
        let sd = advertisementData[CBAdvertisementDataServiceDataKey] as? [CBUUID: Data]
        if let payload = sd?[presenceUUID], payload.count == payloadLen,
           payload[payload.startIndex] == presenceVersion {
            let b = payload.startIndex
            ver = String(payload[b])
            keyId = String(payload[b + 1])
            cmd = String(payload[b + 2])
            let seqBytes = payload.subdata(in: (b + 3)..<(b + 7))
            seq = String(seqBytes.reduce(UInt32(0)) { ($0 << 8) | UInt32($1) })
            tag = hex(payload.subdata(in: (b + 7)..<payload.endIndex))
            if !sawPayload {
                sawPayload = true
                log("PAYLOAD seen: ver=\(ver) keyId=\(keyId) tag=\(tag) — "
                    + "authenticity is presence-verify's call, not this process's")
            }
        }

        print("\(ms),\(RSSI.intValue),\(idPrefix),\(ver),\(keyId),\(tag),\(cmd),\(seq)")
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
        if flag == "--all" { scanAll = true; continue }
        log("usage: rssi-scan [--duration SECONDS] [--all]")
        exit(64)
    }
}

// Die with whoever started us.
//
// The pipeline reaps this process in its exit handler, but an exit handler that
// does not run -- a force-quit app, a SIGKILL, a crash -- leaves a scanner
// holding the Bluetooth session and draining battery for a feature nobody is
// using any more.
//
// What was actually observed: after several rounds of starting and stopping
// pipelines by hand, three rssi-scan processes were still running, listing
// parent pids that no longer existed. Whether they were truly orphaned or the
// parents were mid-teardown was never established -- they were gone a minute
// later. So this guard is written for the failure it prevents rather than as
// the fix for a diagnosis nobody finished, and the honest summary is that
// nothing on any screen would have mentioned a scanner outliving its pipeline
// either way.
//
// getppid() becomes 1 when the parent goes, so the check is one syscall.
let parentAtStart = getppid()
DispatchQueue.global().async {
    while true {
        Thread.sleep(forTimeInterval: 5)
        if getppid() != parentAtStart {
            log("parent went away, exiting rather than scanning for nobody")
            exit(0)
        }
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
