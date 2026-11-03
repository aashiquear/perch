package io.perch.android.touchpad

import java.io.ByteArrayOutputStream

/**
 * Minimal postcard encoder for `perch_proto::StreamPacket` — exact bytes are
 * pinned by `wire_format_tests` in crates/perch-proto/src/lib.rs.
 *
 * Layout: enum variant index as LEB128 varint, then fields in order.
 * f32 little-endian, bool one byte, u16 varint, char as UTF-8 string
 * (varint length + bytes), Vec as varint length + items.
 */
object Postcard {
    const val PKT_POSE = 0
    const val PKT_INPUT = 1
    const val PKT_PING = 2
    const val PKT_PONG = 3

    const val EV_MOUSE_MOVE = 0
    const val EV_MOUSE_MOVE_ABS = 1
    const val EV_MOUSE_BUTTON = 2
    const val EV_SCROLL = 3
    const val EV_KEY = 4
    const val EV_TEXT = 5
    const val EV_GAMEPAD = 6

    const val BTN_LEFT = 0
    const val BTN_RIGHT = 1
    const val BTN_MIDDLE = 2
    const val BTN_BACK = 3
    const val BTN_FORWARD = 4

    /** An input event ready to be batched. */
    sealed class Event {
        data class MouseMove(val dx: Float, val dy: Float) : Event()
        data class MouseButton(val button: Int, val down: Boolean) : Event()
        data class Scroll(val dx: Float, val dy: Float) : Event()
        data class Key(val usage: Int, val down: Boolean, val mods: Int) : Event()
        data class Text(val ch: String) : Event()
        /** Whole controller state (since 1.6): 8 axes (−1..1 sticks, 0..1 triggers, hat −1/0/1) + button bits. */
        data class Gamepad(val axes: FloatArray, val buttons: Int) : Event()
    }

    fun ping(nonce: Int): ByteArray = ByteArrayOutputStream().apply { varint(this, PKT_PING); varint(this, nonce) }.toByteArray()

    fun input(events: List<Event>): ByteArray {
        val out = ByteArrayOutputStream()
        varint(out, PKT_INPUT)
        varint(out, events.size)
        for (e in events) when (e) {
            is Event.MouseMove -> { varint(out, EV_MOUSE_MOVE); f32(out, e.dx); f32(out, e.dy) }
            is Event.MouseButton -> { varint(out, EV_MOUSE_BUTTON); varint(out, e.button); out.write(if (e.down) 1 else 0) }
            is Event.Scroll -> { varint(out, EV_SCROLL); f32(out, e.dx); f32(out, e.dy) }
            is Event.Key -> { varint(out, EV_KEY); varint(out, e.usage); out.write(if (e.down) 1 else 0); out.write(e.mods and 0xff) }
            is Event.Text -> { varint(out, EV_TEXT); val b = e.ch.toByteArray(Charsets.UTF_8); varint(out, b.size); out.write(b) }
            is Event.Gamepad -> { varint(out, EV_GAMEPAD); for (i in 0 until 8) f32(out, e.axes.getOrElse(i) { 0f }); varint(out, e.buttons and 0x7fff) }
        }
        return out.toByteArray()
    }

    private fun varint(out: ByteArrayOutputStream, v: Int) {
        var x = v.toLong() and 0xffffffffL
        while (x >= 0x80) { out.write(((x and 0x7f) or 0x80).toInt()); x = x shr 7 }
        out.write(x.toInt())
    }

    private fun f32(out: ByteArrayOutputStream, f: Float) {
        val bits = java.lang.Float.floatToIntBits(f)
        out.write(bits and 0xff); out.write((bits shr 8) and 0xff); out.write((bits shr 16) and 0xff); out.write((bits shr 24) and 0xff)
    }
}
