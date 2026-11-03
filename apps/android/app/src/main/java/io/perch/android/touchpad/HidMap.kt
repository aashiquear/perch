package io.perch.android.touchpad

import android.view.KeyEvent

/** Android key codes → USB HID usages (page 7), as perchd's keymap expects. */
object HidMap {
    private val table = HashMap<Int, Int>().apply {
        for (i in 0..25) put(KeyEvent.KEYCODE_A + i, 0x04 + i)
        for (i in 1..9) put(KeyEvent.KEYCODE_0 + i, 0x1e + i - 1)
        put(KeyEvent.KEYCODE_0, 0x27)
        put(KeyEvent.KEYCODE_ENTER, 0x28); put(KeyEvent.KEYCODE_ESCAPE, 0x29); put(KeyEvent.KEYCODE_DEL, 0x2a)
        put(KeyEvent.KEYCODE_TAB, 0x2b); put(KeyEvent.KEYCODE_SPACE, 0x2c); put(KeyEvent.KEYCODE_MINUS, 0x2d)
        put(KeyEvent.KEYCODE_EQUALS, 0x2e); put(KeyEvent.KEYCODE_LEFT_BRACKET, 0x2f); put(KeyEvent.KEYCODE_RIGHT_BRACKET, 0x30)
        put(KeyEvent.KEYCODE_BACKSLASH, 0x31); put(KeyEvent.KEYCODE_SEMICOLON, 0x33); put(KeyEvent.KEYCODE_APOSTROPHE, 0x34)
        put(KeyEvent.KEYCODE_GRAVE, 0x35); put(KeyEvent.KEYCODE_COMMA, 0x36); put(KeyEvent.KEYCODE_PERIOD, 0x37)
        put(KeyEvent.KEYCODE_SLASH, 0x38); put(KeyEvent.KEYCODE_CAPS_LOCK, 0x39)
        for (i in 0..11) put(KeyEvent.KEYCODE_F1 + i, 0x3a + i)
        put(KeyEvent.KEYCODE_INSERT, 0x49); put(KeyEvent.KEYCODE_MOVE_HOME, 0x4a); put(KeyEvent.KEYCODE_PAGE_UP, 0x4b)
        put(KeyEvent.KEYCODE_FORWARD_DEL, 0x4c); put(KeyEvent.KEYCODE_MOVE_END, 0x4d); put(KeyEvent.KEYCODE_PAGE_DOWN, 0x4e)
        put(KeyEvent.KEYCODE_DPAD_RIGHT, 0x4f); put(KeyEvent.KEYCODE_DPAD_LEFT, 0x50); put(KeyEvent.KEYCODE_DPAD_DOWN, 0x51)
        put(KeyEvent.KEYCODE_DPAD_UP, 0x52); put(KeyEvent.KEYCODE_DPAD_CENTER, 0x28)
        put(KeyEvent.KEYCODE_CTRL_LEFT, 0xe0); put(KeyEvent.KEYCODE_SHIFT_LEFT, 0xe1); put(KeyEvent.KEYCODE_ALT_LEFT, 0xe2)
        put(KeyEvent.KEYCODE_META_LEFT, 0xe3); put(KeyEvent.KEYCODE_CTRL_RIGHT, 0xe4); put(KeyEvent.KEYCODE_SHIFT_RIGHT, 0xe5)
        put(KeyEvent.KEYCODE_ALT_RIGHT, 0xe6); put(KeyEvent.KEYCODE_META_RIGHT, 0xe7)
        put(KeyEvent.KEYCODE_BACK, 0x29)   // Back on a laptop: Escape
    }
    fun usage(keyCode: Int): Int? = table[keyCode]
    /** perchd modifier bits: 1 ctrl, 2 shift, 4 alt, 8 gui. */
    fun mods(metaState: Int): Int =
        (if (metaState and KeyEvent.META_CTRL_ON != 0) 1 else 0) or (if (metaState and KeyEvent.META_SHIFT_ON != 0) 2 else 0) or
        (if (metaState and KeyEvent.META_ALT_ON != 0) 4 else 0) or (if (metaState and KeyEvent.META_META_ON != 0) 8 else 0)
}
