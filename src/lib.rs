//! Byte-exact formatting macros for C-to-Rust translations.
//!
//! Display placeholders (`{}`) use byte-oriented width and precision for
//! values implementing [`ByteDisplay`]. Other values and all explicitly typed
//! placeholders are delegated to Rust's standard formatting traits.

#![warn(missing_docs)]

use std::borrow::Cow;
use std::ffi::{CStr, CString};
#[cfg(unix)]
use std::ffi::{OsStr, OsString};
use std::io;
#[cfg(unix)]
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

/// Shared borrowed byte views and bounded string destinations.
pub use xj_bytes::{ByteView, CStrBuffer, CStrDest, c_bytes, to_c_str};

/// How a Display (`{}`) placeholder interprets a byte view.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ByteFormat {
    /// Stops at the first NUL. Precision limits the number of bytes examined.
    CString,
    /// Prints literal bytes, including NULs. Precision limits their count.
    Bytes,
    /// Prints a character's bytes, including NUL, and ignores precision.
    Character,
}

/// Chooses formatting behavior for a borrowed byte view.
///
/// Implementing `ByteView` alone does not opt a type into byte formatting.
/// Standard formatting traits are used for other values and explicitly typed
/// placeholders. Width always counts the bytes actually printed.
///
/// ```
/// use xj_cfmt::{ByteDisplay, ByteFormat, ByteView};
/// struct Packet(Vec<u8>);
/// impl ByteView for Packet {
///     fn view_bytes(&self) -> &[u8] { &self.0 }
/// }
/// impl ByteDisplay for Packet {
///     fn byte_format(&self) -> ByteFormat { ByteFormat::Bytes }
/// }
/// let mut output = Vec::new();
/// xj_cfmt::cwrite!(&mut output, "{:.2}", Packet(vec![0xff, 1, 2]));
/// assert_eq!(output, [0xff, 1]);
/// ```
pub trait ByteDisplay: ByteView {
    /// Chooses NUL and precision handling for Display placeholders.
    fn byte_format(&self) -> ByteFormat;
}

/// A single byte formatted with C `%c` semantics.
///
/// ```
/// let mut output = Vec::new();
/// assert_eq!(xj_cfmt::cwrite!(&mut output, "{}", xj_cfmt::Byte(0xff)), 1);
/// assert_eq!(output, [0xff]);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Byte(pub u8);

impl ByteView for Byte {
    fn view_bytes(&self) -> &[u8] {
        std::slice::from_ref(&self.0)
    }
}

impl ByteDisplay for Byte {
    fn byte_format(&self) -> ByteFormat {
        ByteFormat::Character
    }
}

/// Formats a borrowed byte view literally, including NULs.
///
/// Precision and width count bytes. This overrides only Display formatting;
/// consumers such as printf `%s` still apply their own string semantics.
///
/// ```
/// use xj_cfmt::{cwrite, RawBytes};
/// let mut output = Vec::new();
/// cwrite!(&mut output, "{}", RawBytes(b"a\0b"));
/// assert_eq!(output, b"a\0b");
/// ```
#[derive(Clone, Copy, Debug)]
pub struct RawBytes<'a, T: ?Sized>(pub &'a T);

impl<T: ByteView + ?Sized> ByteView for RawBytes<'_, T> {
    fn view_bytes(&self) -> &[u8] {
        self.0.view_bytes()
    }
}

impl<T: ByteView + ?Sized> ByteDisplay for RawBytes<'_, T> {
    fn byte_format(&self) -> ByteFormat {
        ByteFormat::Bytes
    }
}

macro_rules! byte_display {
    ($format:ident, $($ty:ty),* $(,)?) => { $(
        impl ByteDisplay for $ty {
            fn byte_format(&self) -> ByteFormat { ByteFormat::$format }
        }
    )* };
}

byte_display!(CString, [u8], Vec<u8>, CStr, CString);
byte_display!(Bytes, str, String);
#[cfg(unix)]
byte_display!(Bytes, OsStr, OsString, Path, PathBuf);

impl<const N: usize> ByteDisplay for [u8; N] {
    fn byte_format(&self) -> ByteFormat {
        ByteFormat::CString
    }
}

impl<T: ByteDisplay + ?Sized> ByteDisplay for &T {
    fn byte_format(&self) -> ByteFormat {
        T::byte_format(self)
    }
}

impl<T: ByteDisplay + ?Sized> ByteDisplay for &mut T {
    fn byte_format(&self) -> ByteFormat {
        T::byte_format(self)
    }
}

macro_rules! byte_display_pointer {
    ($ty:ident) => {
        impl<T: ByteDisplay + ?Sized> ByteDisplay for $ty<T> {
            fn byte_format(&self) -> ByteFormat {
                T::byte_format(self)
            }
        }
    };
}
byte_display_pointer!(Box);
byte_display_pointer!(Rc);
byte_display_pointer!(Arc);

impl<B: ByteDisplay + ToOwned + ?Sized> ByteDisplay for Cow<'_, B> {
    fn byte_format(&self) -> ByteFormat {
        B::byte_format(self)
    }
}

/// Receives one complete, buffered macro call.
///
/// `cemit!` evaluates its sink first and all arguments second. Unless that
/// evaluation or rendering panics, it calls `emit` exactly once, including for
/// empty output and render errors. No argument or formatting implementation is
/// evaluated while `emit` runs. Rendered prefixes are delivered on errors.
pub trait CSink {
    /// Emits one call's bytes.
    fn emit(self, bytes: &[u8]) -> io::Result<()>;
}

impl<W: io::Write + ?Sized> CSink for &mut W {
    fn emit(self, bytes: &[u8]) -> io::Result<()> {
        self.write_all(bytes)
    }
}

/// A sink for the process's standard output stream.
///
/// This is the sink used by [`cprintf!`].
#[derive(Clone, Copy, Debug, Default)]
pub struct CStdout;

/// A sink for the process's standard error stream.
///
/// This is the sink used by [`ceprintf!`].
#[derive(Clone, Copy, Debug, Default)]
pub struct CStderr;

#[cfg(feature = "std-stdio")]
impl CSink for CStdout {
    fn emit(self, bytes: &[u8]) -> io::Result<()> {
        use io::Write as _;
        io::stdout().lock().write_all(bytes)
    }
}

#[cfg(feature = "std-stdio")]
impl CSink for CStderr {
    fn emit(self, bytes: &[u8]) -> io::Result<()> {
        use io::Write as _;
        io::stderr().lock().write_all(bytes)
    }
}

#[cfg(not(feature = "std-stdio"))]
impl CSink for CStdout {
    fn emit(self, bytes: &[u8]) -> io::Result<()> {
        __private::emit_c_stdio(bytes, __private::CStream::Stdout)
    }
}

#[cfg(not(feature = "std-stdio"))]
impl CSink for CStderr {
    fn emit(self, bytes: &[u8]) -> io::Result<()> {
        __private::emit_c_stdio(bytes, __private::CStream::Stderr)
    }
}

/// Formats to C `stdout`, returning the byte count or `-1`.
///
/// String-like values use byte-counted width and precision. Other values use
/// Rust's standard formatting traits.
///
/// ```no_run
/// let result = xj_cfmt::cprintf!("{:>6}: {}\n", b"name", 4);
/// assert_eq!(result, 13);
/// ```
#[macro_export]
macro_rules! cprintf {
    ($($tokens:tt)*) => {
        $crate::__private::cprintf_impl!($crate; $($tokens)*)
    };
}

/// Formats to C `stderr`, returning the byte count or `-1`.
///
/// ```no_run
/// let _ = xj_cfmt::ceprintf!("error: {}\n", b"failed");
/// ```
#[macro_export]
macro_rules! ceprintf {
    ($($tokens:tt)*) => {
        $crate::__private::ceprintf_impl!($crate; $($tokens)*)
    };
}

/// Buffers output and emits it once through a [`CSink`].
///
/// ```
/// let mut output = Vec::new();
/// assert_eq!(xj_cfmt::cemit!(&mut output, "{}", b"ok"), 2);
/// assert_eq!(output, b"ok");
/// ```
#[macro_export]
macro_rules! cemit {
    ($sink:expr, $($tokens:tt)*) => {
        $crate::__private::cemit_impl!($crate; $sink; $($tokens)*)
    };
}

/// Streams formatted bytes into an [`std::io::Write`].
///
/// ```
/// let mut output = Vec::new();
/// assert_eq!(xj_cfmt::cwrite!(&mut output, "{:>4}|{:04x}", "é", 15), 9);
/// assert_eq!(output, b"  \xc3\xa9|000f");
/// ```
#[macro_export]
macro_rules! cwrite {
    ($destination:expr, $($tokens:tt)*) => {
        $crate::__private::cwrite_impl!($crate; $destination; $($tokens)*)
    };
}

/// Formats with C `snprintf` truncation and return-value semantics.
///
/// ```
/// let mut buffer = [b'x'; 4];
/// assert_eq!(xj_cfmt::csnprintf!(&mut buffer, "hello"), 5);
/// assert_eq!(buffer, *b"hel\0");
/// ```
#[macro_export]
macro_rules! csnprintf {
    ($destination:expr, $($tokens:tt)*) => {
        $crate::__private::csnprintf_impl!($crate; $destination; $($tokens)*)
    };
}

/// Implementation details used by macro expansions.
#[doc(hidden)]
pub mod __private;

/// Shared C format syntax and analysis.
pub use xj_cfmt_core as cformat;
/// C conversion execution and destination adapters.
pub mod printf;

/// C sprintf into a raw buffer. Requires an unsafe caller.
#[macro_export]
macro_rules! sprintf {
    ($dest:expr, $fmt:expr $(, $arg:expr)* $(,)?) => {
        $crate::printf::finish_raw(($dest).cast(), None,
            $crate::printf::render($crate::ByteView::view_bytes(&$fmt), &[$($crate::printf::Adapt::c_arg(&$arg)),*]))
    };
}
/// C snprintf into a raw buffer. Requires an unsafe caller.
#[macro_export]
macro_rules! snprintf {
    ($dest:expr, $size:expr, $fmt:expr $(, $arg:expr)* $(,)?) => {
        $crate::printf::finish_raw(($dest).cast(), Some(($size) as usize),
            $crate::printf::render($crate::ByteView::view_bytes(&$fmt), &[$($crate::printf::Adapt::c_arg(&$arg)),*]))
    };
}
/// C sprintf into safe storage, using safe arguments.
#[macro_export]
macro_rules! sprintf_safe {
    ($dest:expr, $fmt:expr $(, $arg:expr)* $(,)?) => {{
        $crate::__private::validate_safe_format!($fmt);
        use $crate::printf::DestinationExt as _;
        ($dest).__c_finish( None,
            $crate::printf::render_safe($crate::ByteView::view_bytes(&$fmt), &[$($crate::printf::SafeAdapt::safe_arg(&$arg)),*]))
    }};
}
/// C snprintf into safe storage, using safe arguments.
#[macro_export]
macro_rules! snprintf_safe {
    ($dest:expr, $size:expr, $fmt:expr $(, $arg:expr)* $(,)?) => {{
        $crate::__private::validate_safe_format!($fmt);
        use $crate::printf::DestinationExt as _;
        ($dest).__c_finish( Some(($size) as usize),
            $crate::printf::render_safe($crate::ByteView::view_bytes(&$fmt), &[$($crate::printf::SafeAdapt::safe_arg(&$arg)),*]))
    }};
}
/// C sprintf into safe storage with raw arguments. Requires an unsafe caller.
#[macro_export]
macro_rules! sprintf_unchecked {
    ($dest:expr, $fmt:expr $(, $arg:expr)* $(,)?) => {{
        use $crate::printf::DestinationExt as _;
        ($dest).__c_finish( None,
            $crate::printf::render($crate::ByteView::view_bytes(&$fmt), &[$($crate::printf::Adapt::c_arg(&$arg)),*]))
    }};
}
/// C snprintf into safe storage with raw arguments. Requires an unsafe caller.
#[macro_export]
macro_rules! snprintf_unchecked {
    ($dest:expr, $size:expr, $fmt:expr $(, $arg:expr)* $(,)?) => {{
        use $crate::printf::DestinationExt as _;
        ($dest).__c_finish( Some(($size) as usize),
            $crate::printf::render($crate::ByteView::view_bytes(&$fmt), &[$($crate::printf::Adapt::c_arg(&$arg)),*]))
    }};
}
