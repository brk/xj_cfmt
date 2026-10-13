use std::borrow::Cow;
use std::fmt;
use std::rc::Rc;

use xj_cfmt::{Byte, ByteDisplay, ByteFormat, ByteView, RawBytes, cwrite, sprintf};

struct Packet([u8; 3]);

impl ByteView for Packet {
    fn view_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Display for Packet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("packet")
    }
}

#[test]
fn raw_bytes_overrides_display_without_changing_printf_string_semantics() {
    let packet = Packet(*b"a\0b");
    let mut output = Vec::new();
    assert_eq!(
        cwrite!(&mut output, "{}|{:>5.3}", packet, RawBytes(&packet)),
        12
    );
    assert_eq!(output, b"packet|  a\0b");
    assert_eq!(sprintf!(output, c"%s", RawBytes(&packet)), 1);
    assert_eq!(output, b"a");
    assert_eq!(sprintf!(output, c"%s", "a\0b"), 1);
    assert_eq!(output, b"a");
}

#[test]
fn cstr_view_includes_terminator_but_string_display_excludes_it() {
    let value = c"ab";
    assert_eq!(value.view_bytes(), b"ab\0");
    let mut output = Vec::new();
    assert_eq!(
        cwrite!(&mut output, "{:>4}|{}|{:.0}", value, RawBytes(value), value),
        9
    );
    assert_eq!(output, b"  ab|ab\0|");
}

#[test]
fn forwarding_preserves_formatting_policy_and_character_nuls() {
    assert_eq!(b"a\0b".byte_format(), ByteFormat::CString);
    assert_eq!("a\0b".byte_format(), ByteFormat::Bytes);
    let character = Rc::new(Byte(0));
    let text: Cow<'_, str> = Cow::Owned("a\0b".into());
    let mut output = Vec::new();
    assert_eq!(cwrite!(&mut output, "{:>3.0}|{}", character, text), 7);
    assert_eq!(output, b"  \0|a\0b");
    assert_eq!(sprintf!(output, c"%.0c", Byte(0)), 1);
    assert_eq!(output, b"\0");
}
