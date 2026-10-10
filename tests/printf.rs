use std::ffi::CString;
use xj_cfmt::printf::{Arg, render};
use xj_cfmt::{snprintf, snprintf_safe, sprintf_safe};

// Each reference call supplies the actual promoted C argument types.
macro_rules! differential {
    ($fmt:expr $(, $arg:expr)* $(,)?) => {{
        let mut expected = [0xabu8; 1024];
        let format = CString::new($fmt).unwrap();
        let c_count = unsafe { libc::snprintf(expected.as_mut_ptr().cast(), expected.len(), format.as_ptr() $(, $arg)*) };
        assert!(c_count >= 0);
        let mut literal = [0xabu8; 1024];
        let count = unsafe { snprintf!(literal.as_mut_ptr(), literal.len(), $fmt $(, $arg)*) };
        assert_eq!(count, c_count, "{:?}", format);
        assert_eq!(literal, expected, "{:?}", format);
        let mut dynamic = [0xabu8; 1024];
        let count = unsafe { snprintf!(dynamic.as_mut_ptr(), dynamic.len(), format.as_c_str() $(, $arg)*) };
        assert_eq!(count, c_count);
        assert_eq!(dynamic, expected);
    }};
}
#[test]
fn promoted_integer_boundaries_flags_and_lengths() {
    for v in [0i32, 1, -1, i32::MIN, i32::MAX, 128, 255, 32768, 65535] {
        differential!(
            b"%d|%i|%hhd|%hd|%+08d|% 6d|%08.4d|%-08d|%.0d",
            v,
            v,
            v,
            v,
            v,
            v,
            v,
            v,
            v
        );
    }
    for v in [0u32, 1, 255, u32::MAX, 65535] {
        differential!(
            b"%u|%x|%X|%o|%#o|%#.0x|%#08x|%08.4u",
            v,
            v,
            v,
            v,
            v,
            v,
            v,
            v
        );
        differential!(b"%hhu|%hu|%hhx|%ho", v as i32, v as i32, v as i32, v as i32);
    }
    differential!(
        b"%ld|%lu|%lld|%llu|%jd|%ju|%zd|%zu|%td|%tu",
        libc::c_long::MIN,
        libc::c_ulong::MAX,
        i64::MIN,
        u64::MAX,
        libc::intmax_t::MIN,
        libc::uintmax_t::MAX,
        isize::MIN,
        usize::MAX,
        isize::MIN,
        usize::MAX
    );
}
#[test]
fn signed_stars_byte_strings_and_characters() {
    let mut character = [0u8; 2];
    assert_eq!(xj_cfmt::sprintf_safe!(character, b"%c", 'A'), 1);
    assert_eq!(&character, b"A\0");
    for width in [-12i32, 0, 12] {
        for precision in [-1i32, 0, 2, 5] {
            differential!(
                b"%*.*s|%*.*d|%%",
                width,
                precision,
                c"\xff\xc3\xa9".as_ptr(),
                width,
                precision,
                123i32
            );
        }
    }
    differential!(b"%5c|%-5c", 255i32, 0i32);
    // A precision-bounded string needs no readable terminator.
    let bytes = [0xffu8, 1, 2];
    differential!(b"%.3s", bytes.as_ptr());
    let mut out = Vec::new();
    assert_eq!(sprintf_safe!(out, b"%.2s\0%q", &[0xffu8, 1, 2]), 2);
    assert_eq!(out, [0xff, 1]);
}
#[test]
fn floating_boundaries_and_native_pointer_profile() {
    for value in [
        0.0f64,
        -0.0,
        9.99999,
        0.000099999,
        f64::MIN_POSITIVE,
        f64::MAX,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
    ] {
        differential!(
            b"%.6f|%.6e|%.6g|%.0g|%#.0f|%#.0e|%#.6g|%+.8a|%020F",
            value,
            value,
            value,
            value,
            value,
            value,
            value,
            value,
            value
        );
    }
    let value = 1i32;
    differential!(
        b"%p|%20p",
        &value as *const i32,
        core::ptr::null::<core::ffi::c_void>()
    );
}
#[test]
fn all_count_store_widths_and_repeated_destinations() {
    let mut a = 0i8;
    let mut b = 0i16;
    let mut c = 0i32;
    let mut d = 0 as libc::c_long;
    let mut e = 0i64;
    let mut f = 0 as libc::intmax_t;
    let mut g = 0isize;
    let mut h = 0isize;
    let mut bytes = [0u8; 32];
    let fmt = b"a%hhn%hn%n%ln%lln%jn%zn%tnb%n";
    let count = unsafe {
        snprintf!(
            bytes.as_mut_ptr(),
            bytes.len(),
            fmt,
            &mut a as *mut i8,
            &mut b as *mut i16,
            &mut c as *mut i32,
            &mut d as *mut libc::c_long,
            &mut e as *mut i64,
            &mut f as *mut libc::intmax_t,
            &mut g as *mut isize,
            &mut h as *mut isize,
            &mut c as *mut i32
        )
    };
    assert_eq!(count, 2);
    assert_eq!((a, b, c, d, e, f, g, h), (1, 1, 2, 1, 1, 1, 1, 1));
    assert_eq!(&bytes[..3], b"ab\0");
    // Earlier count stores are observed by a later string conversion.
    let mut aliased = [b'x', b'x', 0];
    let ptr = aliased.as_mut_ptr();
    let result =
        unsafe { render(b"%hhn%s", &[Arg::Raw(ptr.cast()), Arg::Raw(ptr.cast())]) }.unwrap();
    assert!(result.is_empty());
    assert_eq!(aliased[0], 0);
}
#[test]
fn counts_truncation_termination_zero_capacity_and_evaluation() {
    for size in 0..=8 {
        let mut actual = [0xabu8; 9];
        let mut expected = actual;
        let a = unsafe { snprintf!(actual.as_mut_ptr(), size, b"abcdef") };
        let b = unsafe { libc::snprintf(expected.as_mut_ptr().cast(), size, c"abcdef".as_ptr()) };
        assert_eq!((a, actual), (b, expected));
    }
    assert_eq!(
        unsafe { snprintf!(core::ptr::null_mut::<u8>(), 0, b"abc") },
        3
    );
    let mut count = 0;
    let mut output = Vec::new();
    assert_eq!(
        sprintf_safe!(
            output,
            b"%d",
            {
                count += 1;
                42
            },
            {
                count += 1;
                0
            }
        ),
        2
    );
    assert_eq!(count, 2);
    assert_eq!(output, b"42");
    let mut bounded = [0xffu8; 4];
    assert_eq!(snprintf_safe!(bounded, 4, b"%s", b"abcdef"), 6);
    assert_eq!(&bounded, b"abc\0");
}
#[test]
fn unsupported_and_mismatched_arguments_are_errors() {
    assert!(unsafe { render(b"%d", &[]) }.is_err());
    assert!(unsafe { render(b"%d", &[Arg::Bytes(b"abc")]) }.is_err());
    assert!(unsafe { render(b"%f", &[Arg::I(2)]) }.is_err());
    assert!(unsafe { render(b"%p", &[Arg::Bytes(b"abc")]) }.is_err());
    for fmt in [
        b"%q".as_slice(),
        b"%1$d",
        b"%ls",
        b"%Lf",
        b"%m",
        b"%2147483648d",
    ] {
        assert!(unsafe { render(fmt, &[Arg::I(1)]) }.is_err());
    }
    let mut out = [0xabu8; 4];
    let fmt = CString::new("%q").unwrap();
    assert_eq!(snprintf_safe!(out, out.len(), fmt.as_c_str()), -1);
    assert_eq!(unsafe { *libc::__errno_location() }, libc::EINVAL);
    assert_eq!(out, [0xab; 4]);
    assert!(matches!(
        unsafe { render(b"a%*s", &[Arg::I(i32::MIN as i64), Arg::Bytes(b"a")]) },
        Err(xj_cfmt::printf::Error::Overflow)
    ));
}
