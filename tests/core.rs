use std::cell::Cell;
use std::fmt;
use std::io::{self, Write};

use xj_cfmt::{Byte, CSink, cemit, ceprintf, cprintf, csnprintf, cwrite};

#[test]
fn standard_and_byte_paths() {
    let mut output = Vec::new();
    let number = 31;
    let text = "é";
    let count = cwrite!(&mut output, "{:04x}|{:>5}|{number}", number, text);
    assert_eq!(output, b"001f|   \xc3\xa9|31");
    assert_eq!(count as usize, output.len());
}

#[test]
fn argument_forms_and_dynamic_counts() {
    let mut output = Vec::new();
    let captured = 7;
    let width = 5usize;
    let precision = 3usize;
    let count = cwrite!(
        &mut output,
        "{1} {0} {named:>width$} {captured} {3:.2$}",
        "abcdef",
        9,
        precision,
        "wxyz",
        named = 4,
        width = width,
    );
    assert_eq!(output, b"9 abcdef     4 7 wxy");
    assert_eq!(count as usize, output.len());
}

#[test]
fn unicode_identifiers_work_in_string_formats() {
    let café = 7;
    let largeur = 4usize;
    let mut output = Vec::new();
    let count = cwrite!(&mut output, "{café} {résumé:>largeur$}", résumé = 3);
    assert_eq!(count, 6);
    assert_eq!(output, b"7    3");
}

#[test]
fn byte_arrays_stop_at_nul_and_byte_ignores_precision() {
    let mut output = Vec::new();
    assert_eq!(cwrite!(&mut output, "{:.4}|{:>3.0}", b"ab\0cd", Byte(0)), 6);
    assert_eq!(output, b"ab|  \0");
}

#[test]
fn snprintf_truncates_counts_and_terminates() {
    let mut buffer = [b'x'; 5];
    assert_eq!(csnprintf!(&mut buffer, "{}-{}", b"abcdef", 12), 9);
    assert_eq!(&buffer, b"abcd\0");

    let mut empty = [];
    assert_eq!(csnprintf!(&mut empty, "hello"), 5);
}

struct BareFormatError;
impl fmt::Display for BareFormatError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("prefix")?;
        Err(fmt::Error)
    }
}

#[test]
fn formatting_errors_return_minus_one_and_keep_prefix() {
    let mut output = Vec::new();
    assert_eq!(cwrite!(&mut output, "x{}y", BareFormatError), -1);
    assert_eq!(output, b"xprefix");

    let mut buffer = [b'x'; 16];
    assert_eq!(csnprintf!(&mut buffer, "x{}y", BareFormatError), -1);
    assert_eq!(&buffer[..8], b"xprefix\0");
}

#[derive(Default)]
struct RecordingSink {
    calls: Cell<usize>,
    bytes: std::cell::RefCell<Vec<u8>>,
}
impl CSink for &RecordingSink {
    fn emit(self, bytes: &[u8]) -> io::Result<()> {
        self.calls.set(self.calls.get() + 1);
        self.bytes.borrow_mut().extend_from_slice(bytes);
        Ok(())
    }
}

#[test]
fn sink_is_called_once_including_on_render_error() {
    let sink = RecordingSink::default();
    assert_eq!(cemit!(&sink, "a{}z", BareFormatError), -1);
    assert_eq!(sink.calls.get(), 1);
    assert_eq!(&*sink.bytes.borrow(), b"aprefix");

    let empty = RecordingSink::default();
    assert_eq!(cemit!(&empty, ""), 0);
    assert_eq!(empty.calls.get(), 1);
}

#[test]
fn pointer_formats_the_original_value() {
    let value = 10u8;
    let pointer = &value as *const u8;
    let mut output = Vec::new();
    cwrite!(&mut output, "{:p}", pointer);
    assert_eq!(output, format!("{pointer:p}").into_bytes());
}

#[test]
fn non_utf8_literals_and_cstr() {
    let cstr = c"hi";
    let mut output = Vec::new();
    cwrite!(&mut output, b"\xff{}", cstr);
    assert_eq!(output, b"\xffhi");
}

struct ShortWriter(Vec<u8>);
impl Write for ShortWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.0.len() >= 3 {
            return Err(io::ErrorKind::Other.into());
        }
        let count = (3 - self.0.len()).min(bytes.len());
        self.0.extend_from_slice(&bytes[..count]);
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn writer_errors_leave_prefix() {
    let mut writer = ShortWriter(Vec::new());
    assert_eq!(cwrite!(&mut writer, "abcdef"), -1);
    assert_eq!(writer.0, b"abc");
}

#[test]
fn standard_formatting_trait_matrix_matches_std() {
    macro_rules! parity {
        ($format:literal, $($argument:expr),* $(,)?) => {{
            let mut actual = Vec::new();
            let count = cwrite!(&mut actual, $format, $($argument),*);
            let expected = format!($format, $($argument),*).into_bytes();
            assert_eq!(actual, expected, "format {}", $format);
            assert_eq!(count as usize, expected.len());
        }};
    }

    parity!("{} {:?}", true, 'é');
    parity!(
        "{:+08} {:#010x} {:#o} {:#b}",
        -12i32,
        48879u32,
        511u16,
        13u8
    );
    parity!("{:X} {:e} {:E}", 48879u64, 12.5f64, 12.5f32);
    parity!("{:x?} {:X?}", [10u8, 11], [10u8, 11]);
    parity!("{0:>1$.2$}", 12.3456f64, 9usize, 2usize);
    parity!("{} {}", 5, 1.5);
}

#[test]
fn ascii_byte_path_matches_std() {
    macro_rules! parity {
        ($format:literal, $value:expr) => {{
            let mut actual = Vec::new();
            cwrite!(&mut actual, $format, $value);
            assert_eq!(actual, format!($format, $value).into_bytes());
        }};
    }
    parity!("{}", "abc");
    parity!("{:8.2}", "abc");
    parity!("{:>8.2}", "abc");
    parity!("{:_<8.2}", "abc");
    parity!("{:-^8.2}", "abc");
}

#[test]
fn arguments_are_evaluated_once_left_to_right() {
    let events = std::cell::RefCell::new(Vec::new());
    let evaluate = |value| {
        events.borrow_mut().push(value);
        value
    };
    let mut output = Vec::new();
    cwrite!(
        &mut output,
        "{0} {0:>1$} {named}",
        evaluate(1),
        evaluate(3) as usize,
        named = evaluate(2),
    );
    assert_eq!(&*events.borrow(), &[1, 3, 2]);
    assert_eq!(output, b"1   1 2");
}

#[test]
fn write_accepts_a_non_mutable_mutable_reference_binding() {
    let mut bytes = Vec::new();
    let writer = &mut bytes;
    assert_eq!(cwrite!(writer, "ok"), 2);
    assert_eq!(bytes, b"ok");
}

#[test]
fn built_in_empty_sinks_are_callable() {
    assert_eq!(cprintf!(""), 0);
    assert_eq!(ceprintf!(""), 0);
}

#[test]
fn byte_strings_match_libc_snprintf() {
    fn libc_string(format: &std::ffi::CStr, source: &[u8]) -> (i32, Vec<u8>) {
        let mut destination = [0u8; 64];
        let result = unsafe {
            libc::snprintf(
                destination.as_mut_ptr().cast(),
                destination.len(),
                format.as_ptr(),
                source.as_ptr().cast::<libc::c_char>(),
            )
        };
        (result, destination[..result as usize].to_vec())
    }

    for source in [
        b"ascii\0".as_slice(),
        b"\xf0\x9f\x98\x80z\0".as_slice(),
        b"\x80\xfe\xff\0".as_slice(),
        b"ab\0hidden\0".as_slice(),
        b"\0".as_slice(),
    ] {
        let (c_count, c_output) = libc_string(c"%10.3s", source);
        let mut output = Vec::new();
        let count = cwrite!(&mut output, "{:>10.3}", source);
        assert_eq!((count, output), (c_count, c_output));

        let (c_count, c_output) = libc_string(c"%-10.3s", source);
        let mut output = Vec::new();
        let count = cwrite!(&mut output, "{:<10.3}", source);
        assert_eq!((count, output), (c_count, c_output));
    }
}

#[test]
fn byte_values_match_libc_percent_c() {
    for value in [0, b'A', 0xff] {
        let mut expected = [0u8; 16];
        let expected_count = unsafe {
            libc::snprintf(
                expected.as_mut_ptr().cast(),
                expected.len(),
                c"%5c".as_ptr(),
                i32::from(value),
            )
        };
        let mut actual = Vec::new();
        let actual_count = cwrite!(&mut actual, "{:>5.0}", Byte(value));
        assert_eq!(actual_count, expected_count);
        assert_eq!(actual, expected[..expected_count as usize]);
    }
}

#[test]
fn snprintf_buffer_sizes_match_libc() {
    for size in 0..=8 {
        let mut expected = [b'x'; 8];
        let expected_count = unsafe {
            libc::snprintf(
                expected.as_mut_ptr().cast(),
                size,
                c"%s-%d".as_ptr(),
                c"abcdef".as_ptr(),
                12i32,
            )
        };
        let mut actual = [b'x'; 8];
        let actual_count = csnprintf!(&mut actual[..size], "{}-{}", b"abcdef", 12i32);
        assert_eq!(actual_count, expected_count, "size {size}");
        assert_eq!(&actual[..size], &expected[..size], "size {size}");
    }
}

#[test]
fn maximum_center_width_reaches_the_writer_without_overflowing() {
    struct FailFirst;
    impl Write for FailFirst {
        fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::Other.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    assert_eq!(cwrite!(FailFirst, "{:^1$}", "x", usize::MAX), -1);
}
