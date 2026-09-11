// state-advertise — tell the phone whether this Mac is locked.
//
// The phone's beacon is one-way: it broadcasts and nothing ever answers. That
// was a deliberate choice (no connectable surface, low power), and the price
// was that the phone could not say anything true about the Mac. It could report
// what it had just done -- "已发出" -- and nothing about what happened next.
// So the phone's screen either stayed silent or invented.
//
// This is the other direction, built the same way: a second one-way beacon,
// from the Mac, carrying one byte of state under the same paired key.
//
//   payload = version(1) ‖ keyId(1) ‖ state(1) ‖ tag(8)        13 bytes
//   msg     = "repose-macstate-v1 beacon" ‖ keyId(1) ‖ counter(8 BE) ‖ state(1)
//
// WHY THIS PROCESS HOLDS NO KEY
//
// Advertising needs the Bluetooth TCC grant, which belongs to the app; the
// presence key is root-only 0600 and must not leave the privileged half. That
// is the same split that keeps rssi-scan out of root. So presence-verify mints
// a tag for BOTH states once per window and prints them; this process reads
// them on stdin, reads the lock state itself, and broadcasts whichever matches.
//
// It can therefore claim nothing it was not handed. With no input it goes
// silent rather than advertising an unauthenticated state -- a beacon the phone
// would reject anyway, but one that would look like a working Mac on a capture.
//
// stdin:  macstate,<keyId>,<counter>,<tagUnlocked>,<tagLocked>   (other lines ignored)
// stderr: human-readable events
//
// Build: swiftc -O -o state-advertise state-advertise.swift -framework CoreBluetooth

import CoreBluetooth
import Foundation
import IOKit

/// 0xFFF7. Distinct from the phone's presence beacon (FFF0) and the pairing
/// service (FFF1) so a scanner looking for one never has to reason about the
/// others.
let macStateUUID = CBUUID(string: "FFF7")
let macStateVersion: UInt8 = 0x01
let windowSeconds: Int64 = 30

func log(_ s: String) {
    let ts = ISO8601DateFormatter().string(from: Date())
    FileHandle.standardError.write("[\(ts)] state-advertise: \(s)\n".data(using: .utf8)!)
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

/// Is the session locked?
///
/// The same oracle tests/e2e/lockstate.sh uses, so the two never disagree about
/// what "locked" means. IOConsoleLocked is readable without root.
func screenIsLocked() -> Bool {
    let root = IORegistryGetRootEntry(kIOMainPortDefault)
    defer { IOObjectRelease(root) }
    var props: Unmanaged<CFMutableDictionary>?
    guard IORegistryEntryCreateCFProperties(root, &props, kCFAllocatorDefault, 0) == KERN_SUCCESS,
          let dict = props?.takeRetainedValue() as? [String: Any] else {
        return false
    }
    return (dict["IOConsoleLocked"] as? Bool) ?? false
}

/// One window's worth of tags, as handed over by the privileged half.
struct Tags {
    let keyId: UInt8
    let counter: Int64
    let unlocked: Data
    let locked: Data
}

final class Advertiser: NSObject, CBPeripheralManagerDelegate {
    private var manager: CBPeripheralManager!
    private var ready = false
    private var tags: Tags?
    private var onAir: (Int64, Bool)?   // (counter, locked) currently advertised

    func start() { manager = CBPeripheralManager(delegate: self, queue: nil) }

    func peripheralManagerDidUpdateState(_ m: CBPeripheralManager) {
        switch m.state {
        case .poweredOn:
            ready = true
            log("bluetooth ready")
            refresh()
        case .unauthorized:
            log("no Bluetooth permission for this process — the parent app's grant is what counts")
            exit(2)
        case .poweredOff:
            log("bluetooth is off")
            ready = false
        default:
            ready = false
        }
    }

    func accept(_ t: Tags) {
        tags = t
        refresh()
    }

    /// Put the right payload on the air, and only when it changes.
    ///
    /// Restarting advertising churns the private address and costs a gap, so it
    /// happens on a state change or a window roll, not on a timer.
    func refresh() {
        guard ready, let t = tags else { return }
        let locked = screenIsLocked()
        if let cur = onAir, cur == (t.counter, locked) { return }

        let tag = locked ? t.locked : t.unlocked
        var payload = Data([macStateVersion, t.keyId, locked ? 1 : 0])
        payload.append(tag)

        manager.stopAdvertising()
        manager.startAdvertising([
            CBAdvertisementDataServiceUUIDsKey: [macStateUUID],
            // CoreBluetooth on macOS does not expose Service Data to a
            // peripheral, so the payload rides in the local name as hex. It is
            // 26 characters, inside the 248-byte limit and inside the 31-byte
            // advertisement once the UUID list is counted.
            CBAdvertisementDataLocalNameKey: payload.map { String(format: "%02x", $0) }.joined(),
        ])
        onAir = (t.counter, locked)
        log("advertising state=\(locked ? "locked" : "unlocked") window=\(t.counter)")
    }
}

setlinebuf(stdout)
let advertiser = Advertiser()
advertiser.start()

// Die with whoever started us: this is one of the processes that used to be
// reparented to launchd and go on running for nobody.
let parentAtStart = getppid()
DispatchQueue.global().async {
    while true {
        Thread.sleep(forTimeInterval: 5)
        if getppid() != parentAtStart {
            log("parent went away, exiting")
            exit(0)
        }
    }
}

// The lock state changes without any input arriving, so it is polled. One
// second is well inside the time it takes someone to walk to their desk, and
// costs one IORegistry read.
DispatchQueue.global().async {
    while true {
        Thread.sleep(forTimeInterval: 1)
        DispatchQueue.main.async { advertiser.refresh() }
    }
}

DispatchQueue.global().async {
    while let line = readLine(strippingNewline: true) {
        let f = line.split(separator: ",", omittingEmptySubsequences: false).map(String.init)
        guard f.count == 5, f[0] == "macstate",
              let keyId = UInt8(f[1]), let counter = Int64(f[2]),
              let unlocked = hexDecode(f[3]), let locked = hexDecode(f[4])
        else { continue }
        let t = Tags(keyId: keyId, counter: counter, unlocked: unlocked, locked: locked)
        DispatchQueue.main.async { advertiser.accept(t) }
    }
    log("input ended; nothing left to authenticate a state with, stopping")
    exit(0)
}

dispatchMain()
