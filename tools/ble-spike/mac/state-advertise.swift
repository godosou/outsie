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
//   payload = version(1) ‖ keyId(1) ‖ macId(2) ‖ state(1) ‖ tag(8)   13 bytes,
//             carried in the local name as unpadded base64url (18 chars)
//   msg     = "repose-macstate-v2 beacon" ‖ keyId(1) ‖ macId(2) ‖ counter(8 BE) ‖ state(1)
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
// stdin:  macstate,<keyId>,<counter>,<tagUnlocked>,<tagLocked>,<macIdHex>
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
let macStateVersion: UInt8 = 0x02
let windowSeconds: Int64 = 30

/// base64url without padding. 13 bytes -> 18 characters.
func base64url(_ d: Data) -> String {
    d.base64EncodedString()
        .replacingOccurrences(of: "+", with: "-")
        .replacingOccurrences(of: "/", with: "_")
        .replacingOccurrences(of: "=", with: "")
}

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
    /// Which Mac this is. Minted by presence-verify, which is the process that
    /// holds the key; this one only relays it, so it cannot claim to be a Mac
    /// it was not handed a tag for.
    let macId: UInt16
    let counter: Int64
    /// Index = state byte: 0 unlocked, 1 locked, 2..7 calibration phases.
    let tags: [Data]
}

/// The calibration phase the app wants on the air, if any (design doc §05).
///
/// A small file the app writes: `<state> <until_ms>`. Read here rather than
/// received on stdin because the app is not the process on the other end of
/// stdin -- the key-holding verifier is -- and the app must not have to go
/// through it to say 「等你走开」. Anything unreadable, out of range or expired
/// means "no phase", and the lock state is advertised as usual.
func calibrationPhase() -> UInt8? {
    guard let path = ProcessInfo.processInfo.environment["REPOSE_PHASE_FILE"],
          let raw = try? String(contentsOfFile: path, encoding: .utf8) else { return nil }
    let f = raw.split(separator: " ", omittingEmptySubsequences: true)
    guard f.count >= 2, let state = UInt8(f[0]), let until = Int64(f[1]), (2...7).contains(state) else { return nil }
    let nowMs = Int64(Date().timeIntervalSince1970 * 1000)
    return (until == 0 || nowMs < until) ? state : nil
}

final class Advertiser: NSObject, CBPeripheralManagerDelegate {
    private var manager: CBPeripheralManager!
    private var ready = false
    /// One set of tags per paired phone's key. Only one can be on the air at a
    /// time, so `current` says which, and `rotateKey` moves it along.
    private var tagsByKey: [UInt8: Tags] = [:]
    private var current: UInt8?
    private var onAir: (UInt8, Int64, UInt8)?  // (keyId, counter, state) on air

    func start() { manager = CBPeripheralManager(delegate: self, queue: nil) }

    /// The one callback that says whether the last startAdvertising actually
    /// worked.
    ///
    /// It was not implemented, and that is how a payload two bytes over the
    /// 31-byte advertisement budget stayed invisible: this process went on
    /// logging "advertising state=unlocked" every window while nothing was on
    /// the air, and the phone, which can only report what it hears, said
    /// 「不知道」 -- the same thing it says when the Mac is asleep.
    func peripheralManagerDidStartAdvertising(_ peripheral: CBPeripheralManager, error: Error?) {
        if let error {
            log("NOT advertising: \(error.localizedDescription)")
            onAir = nil          // so the next window retries rather than
                                 // believing the state is already up
        }
    }

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
        // Keyed, because a Mac paired with two phones holds two keys and has to
        // be verifiable by both. Only one payload can be on the air at a time,
        // so they take turns -- see `rotateKey`.
        tagsByKey[t.keyId] = t
        if current == nil { current = t.keyId }
        refresh()
    }

    /// Move to the next key, if there is more than one.
    ///
    /// A phone forgets the Mac's state after 20 seconds
    /// (SpikeContract.MAC_STATE_STALE_MS), so every key has to come round well
    /// inside that. At one turn every three seconds, six paired phones is still
    /// eighteen. With one key this never fires.
    func rotateKey() {
        guard tagsByKey.count > 1 else { return }
        let ids = tagsByKey.keys.sorted()
        let next = ids.first { $0 > (current ?? 0) } ?? ids.first
        if next != current {
            current = next
            refresh()
        }
    }

    /// Put the right payload on the air, and only when it changes.
    ///
    /// Restarting advertising churns the private address and costs a gap, so it
    /// happens on a state change or a window roll, not on a timer.
    func refresh() {
        guard ready, let id = current, let t = tagsByKey[id] else { return }
        // A calibration in progress takes the byte over from the lock state:
        // the phone is following it, and while you are measuring you are at
        // the Mac anyway.
        let state: UInt8 = calibrationPhase() ?? (screenIsLocked() ? 1 : 0)
        if let cur = onAir, cur == (t.keyId, t.counter, state) { return }
        guard Int(state) < t.tags.count else { return }

        let tag = t.tags[Int(state)]
        var payload = Data([
            macStateVersion,
            t.keyId,
            UInt8((t.macId >> 8) & 0xFF),
            UInt8(t.macId & 0xFF),
            state,
        ])
        payload.append(tag)

        manager.stopAdvertising()
        manager.startAdvertising([
            CBAdvertisementDataServiceUUIDsKey: [macStateUUID],
            // CoreBluetooth on macOS does not expose Service Data to a
            // peripheral, so the payload rides in the local name.
            //
            // BASE64URL, NOT HEX, AND THAT IS NOT A STYLE CHOICE.
            //
            // A legacy advertisement is 31 bytes: 3 for flags, 4 for the 16-bit
            // service UUID list, 2 of header for the name. That leaves 22
            // characters. v1's 11-byte payload was exactly 22 in hex -- full to
            // the brim -- and v2's extra two bytes for the mac id pushed it to
            // 26, which macOS silently refused to put on the air. On the phone
            // that is indistinguishable from a Mac that is switched off.
            //
            // base64url carries the same 13 bytes in 18 characters, unpadded,
            // out of an alphabet no BLE stack will mangle. The encoding is not
            // security-relevant: the tag covers the bytes, not their spelling.
            CBAdvertisementDataLocalNameKey: base64url(payload),
        ])
        onAir = (t.keyId, t.counter, state)
        log("advertising state=\(state) window=\(t.counter) key=\(t.keyId)")
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
    var tick = 0
    while true {
        Thread.sleep(forTimeInterval: 1)
        tick += 1
        DispatchQueue.main.async {
            // Every three seconds, hand the air to the next paired phone's key.
            // A phone forgets this Mac after twenty, so every key has to come
            // round inside that. With one key this does nothing at all.
            if tick % 3 == 0 { advertiser.rotateKey() }
            advertiser.refresh()
        }
    }
}

DispatchQueue.global().async {
    while let line = readLine(strippingNewline: true) {
        let f = line.split(separator: ",", omittingEmptySubsequences: false).map(String.init)
        // Twelve fields: eight tags, one per state the beacon may say. The mac
        // id stays last, so an older verifier's shorter line is rejected
        // outright rather than read as a line with a zero id.
        guard f.count == 12, f[0] == "macstate",
              let keyId = UInt8(f[1]), let counter = Int64(f[2]),
              let macId = UInt16(f[11], radix: 16)
        else { continue }
        let decoded = f[3...10].compactMap(hexDecode)
        guard decoded.count == 8 else { continue }
        let t = Tags(keyId: keyId, macId: macId, counter: counter, tags: decoded)
        DispatchQueue.main.async { advertiser.accept(t) }
    }
    log("input ended; nothing left to authenticate a state with, stopping")
    exit(0)
}

dispatchMain()
