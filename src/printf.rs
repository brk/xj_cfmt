//! Narrow POSIX C formatting. Calls buffer their complete output before writing.
//!
//! Raw arguments are read only when their conversion executes. Callers of the
//! unsafe entry points must supply readable strings (bounded by precision) and
//! aligned, writable count destinations of the type selected by the length.
#![allow(missing_docs)]
use crate::cformat::{Count, Length, Piece, Plan};
use crate::{Byte, ByteView, RawBytes};
use std::ffi::{CStr, CString};
pub use xj_bytes::{CStrOutput as Destination, DestinationSizeError};

#[derive(Clone, Copy)]
pub enum Arg<'a> {
    I(i64),
    U(u64),
    F(f64),
    Bytes(&'a [u8]),
    /// Untyped raw carrier; the conversion determines its interpretation.
    Raw(*mut core::ffi::c_void),
}
// Moving an argument never accesses its pointee. Access requires an unsafe call.
unsafe impl Send for Arg<'_> {}
unsafe impl Sync for Arg<'_> {}

pub trait Adapt {
    fn c_arg(&self) -> Arg<'_>;
}
/// An argument that cannot carry a raw pointer. Its fields are private.
pub struct SafeArg<'a>(Arg<'a>);
pub trait SafeAdapt {
    fn safe_arg(&self) -> SafeArg<'_>;
}
macro_rules! numeric {
    ($variant:ident, $cast:ty, $($ty:ty),*) => { $(
        impl Adapt for $ty { fn c_arg(&self) -> Arg<'_> { Arg::$variant(*self as $cast) } }
        impl SafeAdapt for $ty { fn safe_arg(&self) -> SafeArg<'_> { SafeArg(self.c_arg()) } }
    )* };
}
numeric!(I, i64, i8, i16, i32, i64, isize, bool);
numeric!(U, u64, u8, u16, u32, u64, usize, char);
numeric!(F, f64, f32, f64);
macro_rules! bytes {
    ($($ty:ty),*) => { $(
        impl Adapt for $ty { fn c_arg(&self) -> Arg<'_> { Arg::Bytes(self.view_bytes()) } }
        impl SafeAdapt for $ty { fn safe_arg(&self) -> SafeArg<'_> { SafeArg(self.c_arg()) } }
    )* };
}
bytes!([u8], Vec<u8>, str, String, CStr, CString);
impl Adapt for Byte {
    fn c_arg(&self) -> Arg<'_> {
        Arg::U(u64::from(self.0))
    }
}
impl SafeAdapt for Byte {
    fn safe_arg(&self) -> SafeArg<'_> {
        SafeArg(self.c_arg())
    }
}
impl<T: ByteView + ?Sized> Adapt for RawBytes<'_, T> {
    fn c_arg(&self) -> Arg<'_> {
        Arg::Bytes(self.view_bytes())
    }
}
impl<T: ByteView + ?Sized> SafeAdapt for RawBytes<'_, T> {
    fn safe_arg(&self) -> SafeArg<'_> {
        SafeArg(self.c_arg())
    }
}
impl<const N: usize> Adapt for [u8; N] {
    fn c_arg(&self) -> Arg<'_> {
        Arg::Bytes(self)
    }
}
impl<const N: usize> SafeAdapt for [u8; N] {
    fn safe_arg(&self) -> SafeArg<'_> {
        SafeArg(self.c_arg())
    }
}
impl<T: Adapt + ?Sized> Adapt for &T {
    fn c_arg(&self) -> Arg<'_> {
        (**self).c_arg()
    }
}
impl<T: Adapt + ?Sized> Adapt for &mut T {
    fn c_arg(&self) -> Arg<'_> {
        (**self).c_arg()
    }
}
impl<T: SafeAdapt + ?Sized> SafeAdapt for &T {
    fn safe_arg(&self) -> SafeArg<'_> {
        (**self).safe_arg()
    }
}
impl<T: SafeAdapt + ?Sized> SafeAdapt for &mut T {
    fn safe_arg(&self) -> SafeArg<'_> {
        (**self).safe_arg()
    }
}
impl<T> Adapt for *const T {
    fn c_arg(&self) -> Arg<'_> {
        Arg::Raw(self.cast_mut().cast())
    }
}
impl<T> Adapt for *mut T {
    fn c_arg(&self) -> Arg<'_> {
        Arg::Raw(self.cast())
    }
}
impl SafeArg<'_> {
    /// Adapt borrowed bytes without exposing the raw carrier constructors.
    pub fn bytes(bytes: &[u8]) -> SafeArg<'_> {
        SafeArg(Arg::Bytes(bytes))
    }
}

#[derive(Debug)]
pub enum Error {
    Format(crate::cformat::Error),
    Argument(usize),
    Overflow,
    UnsafeConversion,
    Output,
    Native(i32),
}
impl Error {
    pub fn errno(&self) -> i32 {
        match self {
            Self::Overflow => libc::EOVERFLOW,
            Self::Output => libc::EIO,
            Self::Native(e) => *e,
            _ => libc::EINVAL,
        }
    }
}
impl From<crate::cformat::Error> for Error {
    fn from(e: crate::cformat::Error) -> Self {
        Self::Format(e)
    }
}
/// Sets the native errno used by the selected POSIX profile.
pub fn set_errno(value: i32) {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    unsafe {
        *libc::__errno_location() = value;
    }
    #[cfg(any(target_os = "macos", target_os = "freebsd"))]
    unsafe {
        *libc::__error() = value;
    }
}
fn native_errno() -> i32 {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    unsafe {
        return *libc::__errno_location();
    }
    #[cfg(any(target_os = "macos", target_os = "freebsd"))]
    unsafe {
        return *libc::__error();
    }
    #[allow(unreachable_code)]
    libc::EIO
}
pub fn fail(error: Error) -> i32 {
    set_errno(error.errno());
    -1
}
pub fn count(bytes: &[u8]) -> Result<i32, Error> {
    i32::try_from(bytes.len()).map_err(|_| Error::Overflow)
}

fn integer(arg: Arg<'_>, index: usize) -> Result<u64, Error> {
    match arg {
        Arg::I(v) => Ok(v as u64),
        Arg::U(v) => Ok(v),
        _ => Err(Error::Argument(index)),
    }
}
fn pointer(arg: Arg<'_>, index: usize) -> Result<*mut core::ffi::c_void, Error> {
    match arg {
        Arg::Raw(p) => Ok(p),
        _ => Err(Error::Argument(index)),
    }
}
fn bits(length: Length) -> u32 {
    match length {
        Length::Hh => 8,
        Length::H => 16,
        Length::None => core::ffi::c_int::BITS,
        Length::L => core::ffi::c_long::BITS,
        Length::Ll => core::ffi::c_longlong::BITS,
        Length::J => libc::intmax_t::BITS,
        Length::Z | Length::T => usize::BITS,
        Length::BigL => unreachable!(),
    }
}
fn narrow_unsigned(v: u64, length: Length) -> u64 {
    let n = bits(length);
    if n == 64 { v } else { v & ((1u64 << n) - 1) }
}
fn narrow_signed(v: u64, length: Length) -> i64 {
    let shift = 64 - bits(length);
    ((v << shift) as i64) >> shift
}

/// Executes an already analyzed plan, without reparsing it.
///
/// # Safety
/// Raw pointers must satisfy the requirements described by the module, only
/// during the consuming conversion. Count stores may affect later string reads.
pub unsafe fn render_plan(plan: &Plan<'_>, args: &[Arg<'_>]) -> Result<Vec<u8>, Error> {
    let get = |i: usize| args.get(i).copied().ok_or(Error::Argument(i));
    let mut out = Vec::new();
    for piece in &plan.pieces {
        let Piece::Conversion(c) = piece else {
            let Piece::Literal(bytes) = piece else {
                unreachable!()
            };
            if bytes.len() > i32::MAX as usize - out.len() {
                return Err(Error::Overflow);
            }
            out.extend_from_slice(bytes);
            continue;
        };
        let mut minus = c.flags.minus;
        let mut resolve = |field, width| -> Result<Option<usize>, Error> {
            Ok(match field {
                Count::Absent => None,
                Count::Constant(n) => Some(n as usize),
                Count::Argument(i) => {
                    let n = integer(get(i)?, i)? as i32;
                    if n < 0 {
                        if width {
                            minus = true;
                            Some(n.unsigned_abs() as usize)
                        } else {
                            None
                        }
                    } else {
                        Some(n as usize)
                    }
                }
            })
        };
        let width = resolve(c.width, true)?;
        let precision = resolve(c.precision, false)?;
        if width.is_some_and(|n| n > i32::MAX as usize - out.len())
            || precision.is_some_and(|n| n > i32::MAX as usize)
        {
            return Err(Error::Overflow);
        }
        let index = c.value_argument.unwrap_or(0);
        let zero = c.flags.zero && !minus;
        match c.conversion {
            b'%' => out.push(b'%'),
            b'd' | b'i' => fmt_signed(
                &mut out,
                narrow_signed(integer(get(index)?, index)?, c.length),
                width,
                precision,
                minus,
                c.flags.plus,
                c.flags.space,
                zero,
            ),
            b'u' | b'x' | b'X' => fmt_unsigned(
                &mut out,
                narrow_unsigned(integer(get(index)?, index)?, c.length),
                width,
                precision,
                minus,
                zero,
                if c.conversion == b'u' { 10 } else { 16 },
                c.conversion == b'X',
                c.flags.hash,
            ),
            b'o' => fmt_octal(
                &mut out,
                narrow_unsigned(integer(get(index)?, index)?, c.length),
                width,
                precision,
                minus,
                zero,
                c.flags.hash,
            ),
            b'c' if c.length == Length::L => native_wide_char(
                &mut out,
                c,
                width,
                precision,
                minus,
                integer(get(index)?, index)? as core::ffi::c_uint,
            )?,
            b'c' => pad_and_emit(
                &mut out,
                &[integer(get(index)?, index)? as u8],
                width,
                minus,
                false,
            ),
            b's' => match get(index)? {
                Arg::Bytes(bytes) => {
                    let bytes = &bytes[..bytes.len().min(precision.unwrap_or(usize::MAX))];
                    pad_and_emit(&mut out, crate::c_bytes(bytes), width, minus, false);
                }
                a => {
                    let p = pointer(a, index)?.cast::<u8>();
                    unsafe { fmt_str(&mut out, p, width, precision, minus) };
                }
            },
            b'n' => {
                let p = pointer(get(index)?, index)?;
                if p.is_null() {
                    return Err(Error::Argument(index));
                }
                let n = out.len();
                macro_rules! store {
                    ($ty:ty) => {{
                        let n = <$ty>::try_from(n).map_err(|_| Error::Overflow)?;
                        unsafe {
                            p.cast::<$ty>().write(n);
                        }
                    }};
                }
                match c.length {
                    Length::Hh => store!(i8),
                    Length::H => store!(core::ffi::c_short),
                    Length::None => store!(core::ffi::c_int),
                    Length::L => store!(core::ffi::c_long),
                    Length::Ll => store!(core::ffi::c_longlong),
                    Length::J => store!(libc::intmax_t),
                    Length::Z => store!(libc::ssize_t),
                    Length::T => store!(libc::ptrdiff_t),
                    Length::BigL => unreachable!(),
                }
            }
            b'p' => native_pointer(
                &mut out,
                c,
                width,
                precision,
                minus,
                pointer(get(index)?, index)?,
            )?,
            _ => {
                let Arg::F(v) = get(index)? else {
                    return Err(Error::Argument(index));
                };
                native_float(&mut out, c, width, precision, minus, v)?;
            }
        }
        count(&out)?;
    }
    Ok(out)
}
/// # Safety
/// As for [`render_plan`].
pub unsafe fn render(fmt: &[u8], args: &[Arg<'_>]) -> Result<Vec<u8>, Error> {
    let plan = Plan::new(fmt)?;
    unsafe { render_plan(&plan, args) }
}
/// Safe formatting rejects count stores and accepts only pointer-free carriers.
pub fn render_safe(fmt: &[u8], args: &[SafeArg<'_>]) -> Result<Vec<u8>, Error> {
    let plan = Plan::new(fmt)?;
    if plan.count_stores {
        return Err(Error::UnsafeConversion);
    }
    let args: Vec<_> = args.iter().map(|a| a.0).collect();
    // SafeArg cannot contain raw pointers.
    unsafe { render_plan(&plan, &args) }
}

fn native_format(
    c: &crate::cformat::Conversion,
    width: Option<usize>,
    precision: Option<usize>,
    minus: bool,
) -> CString {
    let mut s = String::from("%");
    for (yes, flag) in [
        (minus, '-'),
        (c.flags.plus, '+'),
        (c.flags.space, ' '),
        (c.flags.hash, '#'),
        (c.flags.zero, '0'),
    ] {
        if yes {
            s.push(flag);
        }
    }
    if let Some(n) = width {
        s.push_str(&n.to_string());
    }
    if let Some(n) = precision {
        s.push('.');
        s.push_str(&n.to_string());
    }
    if c.conversion == b'c' && c.length == Length::L {
        s.push('l');
    }
    s.push(c.conversion as char);
    CString::new(s).unwrap()
}
macro_rules! native {
    ($name:ident, $ty:ty) => {
        fn $name(
            out: &mut Vec<u8>,
            c: &crate::cformat::Conversion,
            width: Option<usize>,
            precision: Option<usize>,
            minus: bool,
            value: $ty,
        ) -> Result<(), Error> {
            let fmt = native_format(c, width, precision, minus);
            // The private format contains exactly one conversion of this type.
            let n = unsafe { libc::snprintf(core::ptr::null_mut(), 0, fmt.as_ptr(), value) };
            if n < 0 {
                return Err(Error::Native(native_errno()));
            }
            let n = n as usize;
            if n > i32::MAX as usize - out.len() {
                return Err(Error::Overflow);
            }
            let mut bytes = vec![0u8; n + 1];
            let written = unsafe {
                libc::snprintf(bytes.as_mut_ptr().cast(), bytes.len(), fmt.as_ptr(), value)
            };
            if written != n as i32 {
                return Err(Error::Output);
            }
            out.extend_from_slice(&bytes[..n]);
            Ok(())
        }
    };
}
native!(native_float, f64);
native!(native_wide_char, core::ffi::c_uint);
native!(native_pointer, *mut core::ffi::c_void);

/// # Safety
/// `ptr` must be writable for min(size, bytes.len()+1) bytes. For None,
/// the complete output and its terminator must fit. No oversized slice is made.
pub unsafe fn write_raw(ptr: *mut u8, size: Option<usize>, bytes: &[u8]) -> Result<(), Error> {
    let size = size.unwrap_or(usize::MAX);
    if size == 0 {
        return Ok(());
    }
    if ptr.is_null() {
        return Err(Error::Argument(0));
    }
    let n = bytes.len().min(size - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, n);
        ptr.add(n).write(0);
    }
    Ok(())
}
pub fn finish<D: Destination + ?Sized>(
    dest: &mut D,
    size: Option<usize>,
    result: Result<Vec<u8>, Error>,
) -> i32 {
    match result.and_then(|bytes| {
        let n = count(&bytes)?;
        dest.write_c(&bytes, size).map_err(|_| Error::Argument(0))?;
        Ok(n)
    }) {
        Ok(n) => n,
        Err(e) => fail(e),
    }
}
/// # Safety
/// As for [`write_raw`].
pub unsafe fn finish_raw(
    dest: *mut u8,
    size: Option<usize>,
    result: Result<Vec<u8>, Error>,
) -> i32 {
    match result.and_then(|bytes| {
        let n = count(&bytes)?;
        unsafe { write_raw(dest, size, &bytes)? };
        Ok(n)
    }) {
        Ok(n) => n,
        Err(e) => fail(e),
    }
}

fn fmt_signed(
    out: &mut Vec<u8>,
    v: i64,
    width: Option<usize>,
    precision: Option<usize>,
    f_minus: bool,
    f_plus: bool,
    f_space: bool,
    f_zero: bool,
) {
    let neg = v < 0;
    let abs = if neg {
        (v as i128).unsigned_abs() as u64
    } else {
        v as u64
    };
    let mut digits = u64_to_decimal(abs);
    if let Some(p) = precision {
        while digits.len() < p {
            digits.insert(0, b'0');
        }
        if v == 0 && p == 0 {
            digits.clear();
        }
    }
    let sign: &[u8] = if neg {
        b"-"
    } else if f_plus {
        b"+"
    } else if f_space {
        b" "
    } else {
        b""
    };

    let total = sign.len() + digits.len();
    let pad = width.map(|w| w.saturating_sub(total)).unwrap_or(0);

    if !f_minus && (!f_zero || precision.is_some()) && pad > 0 {
        for _ in 0..pad {
            out.push(b' ');
        }
    }
    out.extend_from_slice(sign);
    if !f_minus && f_zero && precision.is_none() && pad > 0 {
        for _ in 0..pad {
            out.push(b'0');
        }
    }
    out.extend_from_slice(&digits);
    if f_minus && pad > 0 {
        for _ in 0..pad {
            out.push(b' ');
        }
    }
}

fn fmt_unsigned(
    out: &mut Vec<u8>,
    v: u64,
    width: Option<usize>,
    precision: Option<usize>,
    f_minus: bool,
    f_zero: bool,
    base: u32,
    upper: bool,
    hash: bool,
) {
    let mut digits = u64_to_base(v, base, upper);
    if let Some(p) = precision {
        while digits.len() < p {
            digits.insert(0, b'0');
        }
        if v == 0 && p == 0 {
            digits.clear();
        }
    }
    // Hash for hex: prepend "0x" (or "0X") only if value != 0.
    let prefix: &[u8] = if hash && base == 16 && v != 0 {
        if upper { b"0X" } else { b"0x" }
    } else {
        b""
    };
    let total = prefix.len() + digits.len();
    let pad = width.map(|w| w.saturating_sub(total)).unwrap_or(0);

    if !f_minus && (!f_zero || precision.is_some()) && pad > 0 {
        for _ in 0..pad {
            out.push(b' ');
        }
    }
    out.extend_from_slice(prefix);
    if !f_minus && f_zero && precision.is_none() && pad > 0 {
        for _ in 0..pad {
            out.push(b'0');
        }
    }
    out.extend_from_slice(&digits);
    if f_minus && pad > 0 {
        for _ in 0..pad {
            out.push(b' ');
        }
    }
}

fn fmt_octal(
    out: &mut Vec<u8>,
    v: u64,
    width: Option<usize>,
    precision: Option<usize>,
    f_minus: bool,
    f_zero: bool,
    hash: bool,
) {
    let mut digits = u64_to_base(v, 8, false);
    if let Some(p) = precision {
        while digits.len() < p {
            digits.insert(0, b'0');
        }
        if v == 0 && p == 0 {
            digits.clear();
        }
    }
    if hash {
        // ensure leading 0
        if digits.is_empty() || digits[0] != b'0' {
            digits.insert(0, b'0');
        }
    }
    let total = digits.len();
    let pad = width.map(|w| w.saturating_sub(total)).unwrap_or(0);

    if !f_minus && (!f_zero || precision.is_some()) && pad > 0 {
        for _ in 0..pad {
            out.push(b' ');
        }
    }
    if !f_minus && f_zero && precision.is_none() && pad > 0 {
        for _ in 0..pad {
            out.push(b'0');
        }
    }
    out.extend_from_slice(&digits);
    if f_minus && pad > 0 {
        for _ in 0..pad {
            out.push(b' ');
        }
    }
}

fn u64_to_decimal(mut v: u64) -> Vec<u8> {
    if v == 0 {
        return vec![b'0'];
    }
    let mut out = Vec::with_capacity(20);
    while v > 0 {
        out.push(b'0' + (v % 10) as u8);
        v /= 10;
    }
    out.reverse();
    out
}

fn u64_to_base(mut v: u64, base: u32, upper: bool) -> Vec<u8> {
    if v == 0 {
        return vec![b'0'];
    }
    let lookup = if upper {
        b"0123456789ABCDEF"
    } else {
        b"0123456789abcdef"
    };
    let mut out = Vec::new();
    while v > 0 {
        out.push(lookup[(v % base as u64) as usize]);
        v /= base as u64;
    }
    out.reverse();
    out
}

// ------------- string formatter -------------

/// # Safety
///
/// `p` must be null, or valid for reads up to and including its first
/// NUL, or of `precision` bytes if `precision` is `Some` and no NUL
/// comes first. Those bytes must not be written during the call.
unsafe fn fmt_str(
    out: &mut Vec<u8>,
    p: *const u8,
    width: Option<usize>,
    precision: Option<usize>,
    f_minus: bool,
) {
    let bytes: &[u8] = if p.is_null() {
        b"(null)"
    } else {
        let mut len = 0usize;
        // Walk until NUL or precision cap (precision bounds reads
        // even when no NUL is present, per ISO C).
        let cap = precision.unwrap_or(usize::MAX);
        // SAFETY: the loop reads only bytes before the first NUL or the
        // precision cap (plus the NUL itself), all readable and unwritten
        // per the caller's contract. The slice covers the `len` bytes
        // just read.
        unsafe {
            while len < cap && *p.add(len) != 0 {
                len += 1;
            }
            std::slice::from_raw_parts(p, len)
        }
    };
    let used = match precision {
        Some(p) => bytes.len().min(p),
        None => bytes.len(),
    };
    pad_and_emit(out, &bytes[..used], width, f_minus, false);
}

fn pad_and_emit(
    out: &mut Vec<u8>,
    body: &[u8],
    width: Option<usize>,
    f_minus: bool,
    _f_zero: bool,
) {
    let pad = width.map(|w| w.saturating_sub(body.len())).unwrap_or(0);
    if !f_minus && pad > 0 {
        for _ in 0..pad {
            out.push(b' ');
        }
    }
    out.extend_from_slice(body);
    if f_minus && pad > 0 {
        for _ in 0..pad {
            out.push(b' ');
        }
    }
}

#[doc(hidden)]
pub trait DestinationExt: Destination {
    fn __c_finish(&mut self, size: Option<usize>, result: Result<Vec<u8>, Error>) -> i32 {
        finish(self, size, result)
    }
}
impl<D: Destination + ?Sized> DestinationExt for D {}
