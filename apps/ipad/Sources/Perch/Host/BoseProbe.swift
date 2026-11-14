import CoreBluetooth
import PerchCore
import QuartzCore
import SwiftUI

/// Looks for a Bose AR headset over Bluetooth LE and, if it answers, turns on
/// its rotation sensor and measures what arrives. A check, not yet a tracker:
/// it records raw bytes and the pose at three marked head positions so the
/// axis convention can be established from this headset, not from memory.
@MainActor
final class BoseProbe: NSObject, ObservableObject {
    @Published var lines: [String] = ["not checked yet"]
    @Published var hz = 0.0
    /// Latest rotation (x, y, z, w); not published: it changes 50 times a second.
    private(set) var quat: [Double] = []
    @Published var running = false
    /// Every rotation sample: the sensor's millisecond clock and x, y, z, w.
    var onSample: ((Int, [Double]) -> Void)?

    private var central: CBCentralManager?
    private var peripheral: CBPeripheral?
    private var chars: [String: CBCharacteristic] = [:]
    private var seen: Set<UUID> = []
    private var samples = 0, windowStart = 0.0, rawLogged = 0, configWrites = 0
    private var sensor = BoseAR.Sensor.gameRotation
    private let service = CBUUID(string: BoseAR.service)

    private func log(_ s: String) { lines.append(s); if lines.count > 80 { lines.removeFirst() } }
    private func hex(_ d: Data?) -> String { d.map { $0.map { String(format: "%02x", $0) }.joined(separator: " ") } ?? "nil" }

    func start() {
        stop()
        lines = ["starting Bluetooth…"]
        configWrites = 0; rawLogged = 0
        running = true
        central = CBCentralManager(delegate: self, queue: .main)
    }

    func stop() {
        if let p = peripheral { central?.cancelPeripheralConnection(p) }
        central?.stopScan()
        peripheral = nil; chars = [:]; seen = []; central = nil
        running = false; hz = 0
    }

    /// Record the current pose under a name ("straight", "left", "up").
    func mark(_ name: String) {
        guard quat.count == 4 else { log("mark \(name): no data yet"); return }
        let a = BoseAR.angles(quat)
        log(String(format: "MARK %@: q x %.3f y %.3f z %.3f w %.3f | about Z %.0f° Y %.0f° X %.0f°", name, quat[0], quat[1], quat[2], quat[3], a.aboutZ, a.aboutY, a.aboutX))
    }

    private func connect(_ p: CBPeripheral, why: String) {
        guard peripheral == nil else { return }
        log("connecting to \"\(p.name ?? "?")\" (\(why))")
        peripheral = p
        p.delegate = self
        central?.stopScan()
        central?.connect(p)
    }

    private func enable() {
        guard let p = peripheral, let cfg = chars[BoseAR.sensorConfig], let table = cfg.value else { return }
        let bytes = [UInt8](table)
        // Prefer game rotation (no compass); fall back to rotation.
        for s in [BoseAR.Sensor.gameRotation, .rotation] {
            if let w = BoseAR.config(bytes, enabling: s, periodMs: 20) {
                sensor = s
                configWrites += 1
                log("writing config: \(s) every 20 ms → \(hex(Data(w)))")
                p.writeValue(Data(w), for: cfg, type: .withResponse)
                return
            }
        }
        log("the config table lists no rotation sensor: \(hex(table))")
    }
}

extension BoseProbe: CBCentralManagerDelegate, CBPeripheralDelegate {
    nonisolated func centralManagerDidUpdateState(_ c: CBCentralManager) {
        MainActor.assumeIsolated {
            switch c.state {
            case .poweredOn:
                log("Bluetooth on")
                // Already connected to the iPad through this service?
                let known = c.retrieveConnectedPeripherals(withServices: [service])
                log("connected devices offering the Bose AR service: \(known.count)")
                if let p = known.first { connect(p, why: "already connected"); return }
                log("scanning (12 s)…")
                c.scanForPeripherals(withServices: nil)
                DispatchQueue.main.asyncAfter(deadline: .now() + 12) { [weak self] in
                    guard let self, self.peripheral == nil, self.running else { return }
                    self.central?.stopScan()
                    self.log("scan ended: no device advertised the Bose AR service, \(self.seen.count) Bluetooth LE devices seen")
                }
            case .unauthorized: log("Bluetooth permission denied (Settings → Perch → Bluetooth)")
            case .poweredOff: log("Bluetooth is off")
            case .unsupported: log("Bluetooth LE unsupported")
            default: break
            }
        }
    }

    nonisolated func centralManager(_ c: CBCentralManager, didDiscover p: CBPeripheral, advertisementData ad: [String: Any], rssi: NSNumber) {
        let name = p.name ?? (ad[CBAdvertisementDataLocalNameKey] as? String) ?? ""
        let uuids = (ad[CBAdvertisementDataServiceUUIDsKey] as? [CBUUID] ?? []) + (ad[CBAdvertisementDataOverflowServiceUUIDsKey] as? [CBUUID] ?? [])
        let sdata = (ad[CBAdvertisementDataServiceDataKey] as? [CBUUID: Data])?.keys.map(\.uuidString) ?? []
        let id = p.identifier
        MainActor.assumeIsolated {
            guard seen.insert(id).inserted else { return }
            let hasService = uuids.contains(service) || sdata.contains(BoseAR.service)
            let bose = name.localizedCaseInsensitiveContains("bose")
            if hasService || bose {
                log("found \"\(name)\" rssi \(rssi) services \(uuids.map(\.uuidString)) service-data \(sdata)")
                connect(p, why: hasService ? "advertises Bose AR" : "name")
            }
        }
    }

    nonisolated func centralManager(_ c: CBCentralManager, didConnect p: CBPeripheral) {
        MainActor.assumeIsolated { log("connected; reading services"); p.discoverServices(nil) }
    }

    nonisolated func centralManager(_ c: CBCentralManager, didFailToConnect p: CBPeripheral, error: Error?) {
        MainActor.assumeIsolated { log("connection failed: \(error?.localizedDescription ?? "?")"); peripheral = nil }
    }

    nonisolated func centralManager(_ c: CBCentralManager, didDisconnectPeripheral p: CBPeripheral, error: Error?) {
        MainActor.assumeIsolated {
            log("disconnected\(error.map { ": \($0.localizedDescription)" } ?? "")")
            peripheral = nil; hz = 0; chars = [:]; seen = []; configWrites = 0
            // Headphones switched off or out of range: keep looking while the user wants tracking.
            guard running else { return }
            DispatchQueue.main.asyncAfter(deadline: .now() + 2) { [weak self] in
                guard let self, self.running, self.peripheral == nil, let c = self.central, c.state == .poweredOn else { return }
                self.log("looking for the headphones again…")
                c.scanForPeripherals(withServices: nil)
            }
        }
    }

    nonisolated func peripheral(_ p: CBPeripheral, didDiscoverServices error: Error?) {
        MainActor.assumeIsolated {
            let all = p.services ?? []
            log("services: \(all.map(\.uuid.uuidString).joined(separator: ", "))")
            guard let s = all.first(where: { $0.uuid == service }) else {
                log("NO Bose AR service (FDD2) on this headset: its firmware does not offer head tracking")
                return
            }
            p.discoverCharacteristics(nil, for: s)
        }
    }

    nonisolated func peripheral(_ p: CBPeripheral, didDiscoverCharacteristicsFor s: CBService, error: Error?) {
        MainActor.assumeIsolated {
            for c in s.characteristics ?? [] {
                chars[c.uuid.uuidString] = c
                log("  characteristic \(c.uuid.uuidString.prefix(8)) props 0x\(String(c.properties.rawValue, radix: 16))")
            }
            for id in [BoseAR.deviceInfo, BoseAR.sensorInfo, BoseAR.sensorConfig] { if let c = chars[id] { p.readValue(for: c) } }
            if let d = chars[BoseAR.sensorData] { p.setNotifyValue(true, for: d) } else { log("no sensor-data characteristic") }
        }
    }

    nonisolated func peripheral(_ p: CBPeripheral, didWriteValueFor c: CBCharacteristic, error: Error?) {
        MainActor.assumeIsolated { log("config write: \(error?.localizedDescription ?? "accepted")"); p.readValue(for: c) }
    }

    nonisolated func peripheral(_ p: CBPeripheral, didUpdateNotificationStateFor c: CBCharacteristic, error: Error?) {
        MainActor.assumeIsolated { log("notifications \(c.isNotifying ? "on" : "off")\(error.map { ": \($0.localizedDescription)" } ?? "")") }
    }

    nonisolated func peripheral(_ p: CBPeripheral, didUpdateValueFor c: CBCharacteristic, error: Error?) {
        let value = c.value
        let id = c.uuid.uuidString
        MainActor.assumeIsolated {
            if let error { log("read \(id.prefix(8)): \(error.localizedDescription)"); return }
            guard let value else { return }
            switch id {
            case BoseAR.deviceInfo: log("device info: \(hex(value))")
            case BoseAR.sensorInfo:
                log("sensor info: \(hex(value))")
                log("sample periods by sensor id: \(BoseAR.samplePeriods([UInt8](value)).sorted { $0.key < $1.key })")
            case BoseAR.sensorConfig:
                log("config: \(hex(value)) → \(BoseAR.configEntries([UInt8](value)).map { "\($0.id):\($0.periodMs)ms" })")
                if configWrites == 0 { windowStart = CACurrentMediaTime(); enable() }
            case BoseAR.sensorData:
                if rawLogged < 3 { rawLogged += 1; log("data: \(hex(value))") }
                for r in BoseAR.parse([UInt8](value)) where r.values.count == 4 {
                    samples += 1
                    quat = r.values
                    onSample?(r.timestamp, r.values)
                }
                let now = CACurrentMediaTime()
                if now - windowStart >= 1 {
                    hz = Double(samples) / (now - windowStart)
                    samples = 0; windowStart = now
                }
            default: break
            }
        }
    }
}
