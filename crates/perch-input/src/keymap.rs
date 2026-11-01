//! Key code tables. Perch's wire format uses **USB HID usage ids (page 0x07)**
//! because every OS can map to/from them and they are what a Bluetooth/USB
//! keyboard actually sends. Tables here map HID usage ⇄ Linux evdev
//! `KEY_*` codes; Windows (`VK_*`) and macOS (`kVK_*`) tables go in their
//! backend modules.
//!
//! Source: USB HID Usage Tables 1.4, §10; Linux `input-event-codes.h`.

#![allow(missing_docs)]

/// Commonly used HID usage ids.
pub mod usage {
    pub const KEY_A: u16 = 0x04;
    pub const KEY_Z: u16 = 0x1d;
    pub const KEY_1: u16 = 0x1e;
    pub const KEY_0: u16 = 0x27;
    pub const ENTER: u16 = 0x28;
    pub const ESCAPE: u16 = 0x29;
    pub const BACKSPACE: u16 = 0x2a;
    pub const TAB: u16 = 0x2b;
    pub const SPACE: u16 = 0x2c;
    pub const MINUS: u16 = 0x2d;
    pub const EQUAL: u16 = 0x2e;
    pub const LEFT_BRACKET: u16 = 0x2f;
    pub const RIGHT_BRACKET: u16 = 0x30;
    pub const BACKSLASH: u16 = 0x31;
    pub const SEMICOLON: u16 = 0x33;
    pub const APOSTROPHE: u16 = 0x34;
    pub const GRAVE: u16 = 0x35;
    pub const COMMA: u16 = 0x36;
    pub const DOT: u16 = 0x37;
    pub const SLASH: u16 = 0x38;
    pub const CAPS_LOCK: u16 = 0x39;
    pub const F1: u16 = 0x3a;
    pub const F12: u16 = 0x45;
    pub const PRINT_SCREEN: u16 = 0x46;
    pub const SCROLL_LOCK: u16 = 0x47;
    pub const PAUSE: u16 = 0x48;
    pub const INSERT: u16 = 0x49;
    pub const HOME: u16 = 0x4a;
    pub const PAGE_UP: u16 = 0x4b;
    pub const DELETE: u16 = 0x4c;
    pub const END: u16 = 0x4d;
    pub const PAGE_DOWN: u16 = 0x4e;
    pub const RIGHT: u16 = 0x4f;
    pub const LEFT: u16 = 0x50;
    pub const DOWN: u16 = 0x51;
    pub const UP: u16 = 0x52;
    pub const LEFT_CTRL: u16 = 0xe0;
    pub const LEFT_SHIFT: u16 = 0xe1;
    pub const LEFT_ALT: u16 = 0xe2;
    pub const LEFT_GUI: u16 = 0xe3;
    pub const RIGHT_CTRL: u16 = 0xe4;
    pub const RIGHT_SHIFT: u16 = 0xe5;
    pub const RIGHT_ALT: u16 = 0xe6;
    pub const RIGHT_GUI: u16 = 0xe7;
}

/// Linux evdev key codes for the usages above (values from
/// `linux/input-event-codes.h`). Returns `None` for unmapped usages.
pub fn hid_to_evdev(usage: u16) -> Option<u16> {
    use usage::*;
    Some(match usage {
        // Letters: HID a..z = 0x04..0x1d; evdev KEY_A=30 KEY_B=48 ... not contiguous, use table.
        0x04..=0x1d => LETTERS[(usage - 0x04) as usize],
        // Digits 1..9,0 : HID 0x1e..0x27; evdev KEY_1=2 .. KEY_9=10, KEY_0=11
        0x1e..=0x27 => 2 + (usage - 0x1e),
        ENTER => 28,
        ESCAPE => 1,
        BACKSPACE => 14,
        TAB => 15,
        SPACE => 57,
        MINUS => 12,
        EQUAL => 13,
        LEFT_BRACKET => 26,
        RIGHT_BRACKET => 27,
        BACKSLASH => 43,
        SEMICOLON => 39,
        APOSTROPHE => 40,
        GRAVE => 41,
        COMMA => 51,
        DOT => 52,
        SLASH => 53,
        CAPS_LOCK => 58,
        // F1..F10 = 59..68, F11 = 87, F12 = 88
        0x3a..=0x43 => 59 + (usage - 0x3a),
        0x44 => 87,
        0x45 => 88,
        PRINT_SCREEN => 99,
        SCROLL_LOCK => 70,
        PAUSE => 119,
        INSERT => 110,
        HOME => 102,
        PAGE_UP => 104,
        DELETE => 111,
        END => 107,
        PAGE_DOWN => 109,
        RIGHT => 106,
        LEFT => 105,
        DOWN => 108,
        UP => 103,
        LEFT_CTRL => 29,
        LEFT_SHIFT => 42,
        LEFT_ALT => 56,
        LEFT_GUI => 125,
        RIGHT_CTRL => 97,
        RIGHT_SHIFT => 54,
        RIGHT_ALT => 100,
        RIGHT_GUI => 126,
        _ => return None,
    })
}

/// Inverse of [`hid_to_evdev`].
pub fn evdev_to_hid(code: u16) -> Option<u16> {
    (0x04u16..=0xe7).find(|&u| hid_to_evdev(u) == Some(code))
}

/// evdev codes for KEY_A..KEY_Z in alphabetical order.
const LETTERS: [u16; 26] = [
    30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47, 17, 45, 21, 44,
];

/// Map an ASCII/Unicode char to (usage, needs_shift) for US layout. Used when
/// a phone sends `Text` and the sink has no better IME path.
pub fn char_to_usage_us(c: char) -> Option<(u16, bool)> {
    use usage::*;
    let lower = c.to_ascii_lowercase();
    Some(match lower {
        'a'..='z' => (KEY_A + (lower as u8 - b'a') as u16, c.is_ascii_uppercase()),
        '1'..='9' => (KEY_1 + (lower as u8 - b'1') as u16, false),
        '0' => (KEY_0, false),
        ' ' => (SPACE, false),
        '\n' => (ENTER, false),
        '\t' => (TAB, false),
        '-' => (MINUS, false),
        '_' => (MINUS, true),
        '=' => (EQUAL, false),
        '+' => (EQUAL, true),
        '[' => (LEFT_BRACKET, false),
        '{' => (LEFT_BRACKET, true),
        ']' => (RIGHT_BRACKET, false),
        '}' => (RIGHT_BRACKET, true),
        '\\' => (BACKSLASH, false),
        '|' => (BACKSLASH, true),
        ';' => (SEMICOLON, false),
        ':' => (SEMICOLON, true),
        '\'' => (APOSTROPHE, false),
        '"' => (APOSTROPHE, true),
        '`' => (GRAVE, false),
        '~' => (GRAVE, true),
        ',' => (COMMA, false),
        '<' => (COMMA, true),
        '.' => (DOT, false),
        '>' => (DOT, true),
        '/' => (SLASH, false),
        '?' => (SLASH, true),
        '!' => (KEY_1, true),
        '@' => (KEY_1 + 1, true),
        '#' => (KEY_1 + 2, true),
        '$' => (KEY_1 + 3, true),
        '%' => (KEY_1 + 4, true),
        '^' => (KEY_1 + 5, true),
        '&' => (KEY_1 + 6, true),
        '*' => (KEY_1 + 7, true),
        '(' => (KEY_1 + 8, true),
        ')' => (KEY_0, true),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_roundtrip() {
        for u in usage::KEY_A..=usage::KEY_Z {
            let e = hid_to_evdev(u).unwrap();
            assert_eq!(evdev_to_hid(e), Some(u));
        }
        assert_eq!(hid_to_evdev(usage::KEY_A), Some(30)); // KEY_A
        assert_eq!(hid_to_evdev(usage::ENTER), Some(28)); // KEY_ENTER
        assert_eq!(hid_to_evdev(0x45), Some(88)); // F12
    }

    #[test]
    fn chars() {
        assert_eq!(char_to_usage_us('a'), Some((usage::KEY_A, false)));
        assert_eq!(char_to_usage_us('A'), Some((usage::KEY_A, true)));
        assert_eq!(char_to_usage_us('!'), Some((usage::KEY_1, true)));
        assert_eq!(char_to_usage_us('é'), None);
    }
}
