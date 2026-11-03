package io.perch.android.shell

import android.system.Os
import android.system.OsConstants
import android.util.Log
import java.io.FileDescriptor
import java.nio.ByteBuffer
import java.nio.ByteOrder

/**
 * A virtual HID gamepad through /dev/uhid (same route scrcpy uses, uid shell
 * may open it). Perch never writes input reports to it: it exists so that
 * `InputDevice.getDevice(id)` answers with a real gamepad, which games check
 * before they accept joystick events. The events themselves are injected
 * per display with that device id.
 */
object UhidGamepad {
    private const val TAG = "UhidGamepad"
    const val NAME = "Perch Gamepad"
    private const val UHID_CREATE2 = 11
    private const val UHID_DESTROY = 1
    private const val BUS_BLUETOOTH = 0x05
    private var fd: FileDescriptor? = null

    /**
     * Generic Desktop / Gamepad: X Y Z Rz 16-bit sticks, Brake + Accelerator
     * 8-bit triggers (→ AXIS_BRAKE/GAS on Android), hat, 15 buttons (→ BTN_A …
     * BTN_THUMBR, Android's Generic.kl).
     */
    private val descriptor: ByteArray = intArrayOf(
        0x05, 0x01, 0x09, 0x05, 0xA1, 0x01,             // Usage Page GD, Usage Gamepad, Collection App
        0x09, 0x01, 0xA1, 0x00,                         //   Usage Pointer, Collection Physical
        0x09, 0x30, 0x09, 0x31, 0x09, 0x32, 0x09, 0x35, //     X Y Z Rz
        0x16, 0x00, 0x80, 0x26, 0xFF, 0x7F,             //     logical -32768..32767
        0x75, 0x10, 0x95, 0x04, 0x81, 0x02,             //     16 bit × 4
        0xC0,
        0x05, 0x02, 0x09, 0xC5, 0x09, 0xC4,             //   Simulation: Brake, Accelerator
        0x15, 0x00, 0x26, 0xFF, 0x00, 0x75, 0x08, 0x95, 0x02, 0x81, 0x02,
        0x05, 0x01, 0x09, 0x39,                         //   Hat switch
        0x15, 0x01, 0x25, 0x08, 0x35, 0x00, 0x46, 0x3B, 0x01, 0x65, 0x14,
        0x75, 0x08, 0x95, 0x01, 0x81, 0x42,
        0x05, 0x09, 0x19, 0x01, 0x29, 0x0F,             //   Buttons 1..15
        0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x0F, 0x81, 0x02,
        0x75, 0x01, 0x95, 0x01, 0x81, 0x03,             //   pad
        0xC0,
    ).map { it.toByte() }.toByteArray()

    /** Returns null on success or the failure text. */
    @Synchronized
    fun create(): String? {
        if (fd != null) return null
        return try {
            val f = Os.open("/dev/uhid", OsConstants.O_RDWR or OsConstants.O_CLOEXEC, 0)
            val buf = ByteBuffer.allocate(4380).order(ByteOrder.nativeOrder())
            buf.putInt(UHID_CREATE2)
            val name = NAME.toByteArray()
            buf.put(name); buf.position(4 + 128)              // name[128]
            buf.position(4 + 128 + 64 + 64)                   // phys[64], uniq[64] left empty
            buf.putShort(descriptor.size.toShort())
            buf.putShort(BUS_BLUETOOTH.toShort())
            buf.putInt(0x045E); buf.putInt(0x02FD); buf.putInt(1); buf.putInt(0)   // vendor, product, version, country
            buf.put(descriptor)
            Os.write(f, buf.array(), 0, buf.capacity())
            fd = f
            Log.i(TAG, "uhid gamepad created")
            null
        } catch (e: Throwable) {
            Log.e(TAG, "uhid: $e"); "uhid: $e"
        }
    }

    @Synchronized
    fun destroy() {
        val f = fd ?: return
        runCatching {
            val buf = ByteBuffer.allocate(4).order(ByteOrder.nativeOrder()).putInt(UHID_DESTROY)
            Os.write(f, buf.array(), 0, 4)
            Os.close(f)
        }
        fd = null
    }
}
