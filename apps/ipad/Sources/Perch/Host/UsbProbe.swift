import Darwin
import Foundation

/// Asks iPadOS, at run time, whether this app may see and open the glasses'
/// USB sensor device (Rokid: VID 0x04d2, PID 0x162f, IMU on an interrupt
/// endpoint; docs/DEVICE-ROKID-MAX.md). Nothing here is assumed: every line of
/// the report is what the system answered on this device.
///
/// IOKit is reached through `dlsym` so the probe builds whatever the SDK's
/// headers expose; it is a diagnostic, not a shipping code path.
/// Selectors of IOUSBHostObject / IOUSBHostInterface / IOUSBHostPipe, called
/// dynamically because the framework has no iOS headers.
@objc private protocol UsbHostObject {
    @objc(initWithIOService:options:queue:error:interestHandler:)
    func create(service: UInt32, options: UInt, queue: DispatchQueue?, error: NSErrorPointer,
                interestHandler: (@convention(block) (AnyObject, UInt32, UnsafeMutableRawPointer?) -> Void)?) -> AnyObject?
    @objc(copyPipeWithAddress:error:)
    func pipe(address: UInt, error: NSErrorPointer) -> AnyObject?
    @objc(sendIORequestWithData:bytesTransferred:completionTimeout:error:)
    func read(data: NSMutableData?, transferred: UnsafeMutablePointer<UInt>?, timeout: TimeInterval, error: NSErrorPointer) -> Bool
}

enum UsbProbe {
    static let rokidVendor = 0x04d2

    private typealias Matching = @convention(c) (UnsafePointer<CChar>) -> UnsafeMutableRawPointer?
    private typealias GetServices = @convention(c) (UInt32, UnsafeMutableRawPointer?, UnsafeMutablePointer<UInt32>) -> Int32
    private typealias IterNext = @convention(c) (UInt32) -> UInt32
    private typealias Release = @convention(c) (UInt32) -> Int32
    private typealias CreateProp = @convention(c) (UInt32, CFString, CFAllocator?, UInt32) -> UnsafeMutableRawPointer?
    private typealias GetName = @convention(c) (UInt32, UnsafeMutablePointer<CChar>) -> Int32
    private typealias ServiceOpen = @convention(c) (UInt32, UInt32, UInt32, UnsafeMutablePointer<UInt32>) -> Int32
    private typealias ServiceClose = @convention(c) (UInt32) -> Int32
    private typealias HidCreate = @convention(c) (CFAllocator?, UInt32) -> UnsafeMutableRawPointer?
    private typealias HidSetMatching = @convention(c) (UnsafeMutableRawPointer, UnsafeMutableRawPointer?) -> Void
    private typealias HidOpen = @convention(c) (UnsafeMutableRawPointer, UInt32) -> Int32
    private typealias HidCopyDevices = @convention(c) (UnsafeMutableRawPointer) -> UnsafeMutableRawPointer?
    private typealias HidDevProp = @convention(c) (UnsafeMutableRawPointer, CFString) -> UnsafeMutableRawPointer?

    private static func sym<T>(_ lib: UnsafeMutableRawPointer, _ name: String, as _: T.Type) -> T? {
        guard let p = dlsym(lib, name) else { return nil }
        return unsafeBitCast(p, to: T.self)
    }

    private static func hex(_ r: Int32) -> String { String(format: "0x%08x", UInt32(bitPattern: r)) }

    private static func describe(_ r: Int32) -> String {
        switch UInt32(bitPattern: r) {
        case 0: return "ok"
        case 0xe00002c1: return "\(hex(r)) not privileged"
        case 0xe00002e2: return "\(hex(r)) not permitted (sandbox)"
        case 0xe00002c5: return "\(hex(r)) exclusive access"
        case 0xe00002c7: return "\(hex(r)) unsupported"
        case 0xe00002bc: return "\(hex(r)) general error"
        default: return hex(r)
        }
    }

    private static let hostFramework = dlopen("/System/Library/Frameworks/IOUSBHost.framework/IOUSBHost", RTLD_NOW)

    /// The user-space USB route (IOUSBHost): create the host object for a
    /// registry entry and, for an interface, try to read the IMU endpoint.
    private static func hostObject(_ service: UInt32, interface: Bool, usable: inout Bool) -> String {
        guard hostFramework != nil else { return "framework absent" }
        guard let cls = NSClassFromString(interface ? "IOUSBHostInterface" : "IOUSBHostDevice") as? NSObject.Type else { return "class absent" }
        let initSel = NSSelectorFromString("initWithIOService:options:queue:error:interestHandler:")
        guard cls.instancesRespond(to: initSel), let allocated = cls.perform(NSSelectorFromString("alloc"))?.takeUnretainedValue() else {
            return "initialiser absent"
        }
        var err: NSError?
        guard let obj = unsafeBitCast(allocated, to: UsbHostObject.self)
            .create(service: service, options: 0, queue: nil, error: &err, interestHandler: nil) else {
            return "refused (\(err.map { "\($0.domain) \($0.code) \($0.localizedDescription)" } ?? "no error given"))"
        }
        guard interface else { return "device object created" }
        let pipeSel = NSSelectorFromString("copyPipeWithAddress:error:")
        guard (obj as? NSObject)?.responds(to: pipeSel) == true,
              let pipe = unsafeBitCast(obj, to: UsbHostObject.self).pipe(address: 0x82, error: &err) else {
            return "interface opened, no endpoint 0x82 here"
        }
        let ioSel = NSSelectorFromString("sendIORequestWithData:bytesTransferred:completionTimeout:error:")
        guard (pipe as? NSObject)?.responds(to: ioSel) == true, let buf = NSMutableData(length: 64) else { return "pipe 0x82 found, cannot read" }
        var got = 0, reports = 0
        let start = Date()
        while Date().timeIntervalSince(start) < 0.5 {
            var n: UInt = 0
            if !unsafeBitCast(pipe, to: UsbHostObject.self).read(data: buf, transferred: &n, timeout: 0.2, error: &err) { break }
            reports += 1; got = Int(n)
        }
        if reports > 0 {
            usable = true
            return "READING IMU: \(reports) reports in 0.5 s (\(reports * 2) Hz), \(got) bytes, type \(buf.bytes.load(as: UInt8.self))"
        }
        return "pipe 0x82 found, read failed (\(err?.localizedDescription ?? "timeout"))"
    }

    /// Run every check; returns report lines and whether any route can read the device.
    static func run() -> (lines: [String], usable: Bool) {
        var out: [String] = []
        var usable = false
        guard let lib = dlopen("/System/Library/Frameworks/IOKit.framework/IOKit", RTLD_NOW) else {
            return (["IOKit: cannot be loaded (\(String(cString: dlerror())))"], false)
        }
        out.append("IOKit: loaded")

        // --- registry: can the app see USB devices at all? ---
        if let matching = sym(lib, "IOServiceMatching", as: Matching.self),
           let getServices = sym(lib, "IOServiceGetMatchingServices", as: GetServices.self),
           let next = sym(lib, "IOIteratorNext", as: IterNext.self),
           let release = sym(lib, "IOObjectRelease", as: Release.self),
           let prop = sym(lib, "IORegistryEntryCreateCFProperty", as: CreateProp.self),
           let getName = sym(lib, "IORegistryEntryGetName", as: GetName.self) {
            let open = sym(lib, "IOServiceOpen", as: ServiceOpen.self)
            let close = sym(lib, "IOServiceClose", as: ServiceClose.self)
            func number(_ e: UInt32, _ key: String) -> Int? {
                guard let p = prop(e, key as CFString, nil, 0) else { return nil }
                return (Unmanaged<AnyObject>.fromOpaque(p).takeRetainedValue() as? NSNumber)?.intValue
            }
            for cls in ["IOUSBHostDevice", "IOUSBHostInterface", "IOUSBDevice", "IOHIDDevice"] {
                var iter: UInt32 = 0
                let r = cls.withCString { getServices(0, matching($0), &iter) }
                guard r == 0 else { out.append("registry \(cls): \(describe(r))"); continue }
                var n = 0
                while case let e = next(iter), e != 0 {
                    n += 1
                    var nameBuf = [CChar](repeating: 0, count: 128)
                    _ = getName(e, &nameBuf)
                    let name = String(cString: nameBuf)
                    let vid = number(e, "idVendor") ?? number(e, "VendorID")
                    let pid = number(e, "idProduct") ?? number(e, "ProductID")
                    var line = "  \(cls) \"\(name)\""
                    if let vid, let pid { line += String(format: " vid 0x%04x pid 0x%04x", vid, pid) }
                    let usb = cls.hasPrefix("IOUSB")
                    if vid == nil, usb { line += " (ids hidden from the app)" }
                    // iPadOS hides the ids, so go by name; with one USB device attached
                    // every USB interface belongs to it.
                    if usb, vid == rokidVendor || name.contains("Rokid") || cls == "IOUSBHostInterface", let open, let close {
                        var conn: UInt32 = 0
                        let or = open(e, mach_task_self_, 0, &conn)
                        line += " → open: \(describe(or))"
                        if or == 0 { _ = close(conn) }
                        if cls != "IOUSBDevice" { line += " · IOUSBHost: \(hostObject(e, interface: cls == "IOUSBHostInterface", usable: &usable))" }
                    }
                    if n <= 12 { out.append(line) }
                    _ = release(e)
                }
                _ = release(iter)
                out.append("registry \(cls): \(n) visible")
            }
        } else {
            out.append("registry: IOService symbols missing")
        }

        out.append("IOUSBHost framework on this system: \(hostFramework != nil)")

        // --- HID manager: the route a HID-class device would use ---
        if let create = sym(lib, "IOHIDManagerCreate", as: HidCreate.self),
           let setMatching = sym(lib, "IOHIDManagerSetDeviceMatching", as: HidSetMatching.self),
           let open = sym(lib, "IOHIDManagerOpen", as: HidOpen.self),
           let copy = sym(lib, "IOHIDManagerCopyDevices", as: HidCopyDevices.self),
           let devProp = sym(lib, "IOHIDDeviceGetProperty", as: HidDevProp.self),
           let devOpen = sym(lib, "IOHIDDeviceOpen", as: HidOpen.self) {
            if let mgr = create(nil, 0) {
                setMatching(mgr, nil)
                out.append("HID manager open: \(describe(open(mgr, 0)))")
                if let setPtr = copy(mgr) {
                    let set = Unmanaged<CFSet>.fromOpaque(setPtr).takeRetainedValue()
                    let count = CFSetGetCount(set)
                    var values = [UnsafeRawPointer?](repeating: nil, count: count)
                    CFSetGetValues(set, &values)
                    out.append("HID devices visible: \(count)")
                    for v in values.compactMap({ $0 }) where false {
                        let dev = UnsafeMutableRawPointer(mutating: v)
                        func p(_ k: String) -> AnyObject? {
                            devProp(dev, k as CFString).map { Unmanaged<AnyObject>.fromOpaque($0).takeUnretainedValue() }
                        }
                        let vid = (p("VendorID") as? NSNumber)?.intValue ?? -1
                        let pid = (p("ProductID") as? NSNumber)?.intValue ?? -1
                        var line = String(format: "  HID \"%@\" vid 0x%04x pid 0x%04x %@", (p("Product") as? String) ?? "?", vid, pid,
                                          (p("Transport") as? String) ?? "")
                        if vid == rokidVendor {
                            let r = devOpen(dev, 0)
                            line += " ← ROKID, open: \(describe(r))"
                            if r == 0 { usable = true }
                        }
                        out.append(line)
                    }
                } else {
                    out.append("HID devices visible: none (the system returned no set)")
                }
            } else {
                out.append("HID manager: could not be created")
            }
        } else {
            out.append("HID manager: symbols missing")
        }
        return (out, usable)
    }
}
