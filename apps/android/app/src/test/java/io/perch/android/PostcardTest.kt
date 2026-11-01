package io.perch.android

import io.perch.android.touchpad.Postcard
import org.junit.Assert.assertArrayEquals
import org.junit.Test

/** Mirrors `wire_format_tests` in crates/perch-proto. */
class PostcardTest {
    @Test fun ping() {
        assertArrayEquals(byteArrayOf(2, 1), Postcard.ping(1))
        assertArrayEquals(byteArrayOf(2, 0xAC.toByte(), 0x02), Postcard.ping(300))
    }

    @Test fun inputBatch() {
        val b = Postcard.input(listOf(
            Postcard.Event.MouseMove(1f, -2f),
            Postcard.Event.MouseButton(Postcard.BTN_RIGHT, true),
            Postcard.Event.Scroll(0f, 3f),
            Postcard.Event.Key(0x04, false, 0b0011),
            Postcard.Event.Text("é"),
        ))
        val expected = intArrayOf(
            1, 5,
            0, 0x00, 0x00, 0x80, 0x3F, 0x00, 0x00, 0x00, 0xC0,
            2, 1, 1,
            3, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x40,
            4, 0x04, 0, 0x03,
            5, 2, 0xC3, 0xA9,
        ).map { it.toByte() }.toByteArray()
        assertArrayEquals(expected, b)
    }
}
