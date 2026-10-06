//! Byte-exact formatting macros for C-to-Rust translations.
//!
//! Display placeholders (`{}`) use byte-oriented width and precision for
//! values implementing [`CBytes`]. Other values and all explicitly typed
//! placeholders are delegated to Rust's standard formatting traits.

#![warn(missing_docs)]

use std::borrow::Cow;
use std::ffi::{CStr, CString, OsStr, OsString};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

/// Supplies the bytes printed by a Display (`{}`) placeholder.
///
/// ```
/// struct Packet(Vec<u8>);
/// impl xj_cfmt::CBytes for Packet {
///     fn c_bytes(&self) -> &[u8] { &self.0 }
/// }
/// let mut output = Vec::new();
/// xj_cfmt::cwrite!(&mut output, "{:.2}", Packet(vec![0xff, 1, 2]));
/// assert_eq!(output, [0xff, 1]);
/// ```
pub trait CBytes {
    /// Returns the bytes to print, before precision and NUL handling.
    fn c_bytes(&self) -> &[u8];

    /// Whether output ends at the first NUL byte.
    fn stops_at_nul(&self) -> bool {
        false
    }

    /// Whether precision is ignored, as it is for C's `%c`.
    fn ignores_precision(&self) -> bool {
        false
    }
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

impl CBytes for Byte {
    fn c_bytes(&self) -> &[u8] {
        std::slice::from_ref(&self.0)
    }

    fn ignores_precision(&self) -> bool {
        true
    }
}

macro_rules! c_bytes_slice {
    ($ty:ty, |$value:ident| $body:block, $nul:expr) => {
        impl CBytes for $ty {
            fn c_bytes(&self) -> &[u8] {
                #[allow(clippy::ptr_arg)]
                fn get($value: &$ty) -> &[u8] $body
                get(self)
            }
            fn stops_at_nul(&self) -> bool { $nul }
        }
    };
}

c_bytes_slice!([u8], |value| { value }, true);
c_bytes_slice!(str, |value| { value.as_bytes() }, false);
c_bytes_slice!(Vec<u8>, |value| { value.as_slice() }, true);
c_bytes_slice!(String, |value| { value.as_bytes() }, false);
c_bytes_slice!(CStr, |value| { value.to_bytes() }, false);
c_bytes_slice!(CString, |value| { value.as_bytes() }, false);
c_bytes_slice!(OsStr, |value| { value.as_bytes() }, false);
c_bytes_slice!(OsString, |value| { value.as_os_str().as_bytes() }, false);
c_bytes_slice!(Path, |value| { value.as_os_str().as_bytes() }, false);
c_bytes_slice!(PathBuf, |value| { value.as_os_str().as_bytes() }, false);

impl<const N: usize> CBytes for [u8; N] {
    fn c_bytes(&self) -> &[u8] {
        self
    }
    fn stops_at_nul(&self) -> bool {
        true
    }
}

impl<T: CBytes + ?Sized> CBytes for &T {
    fn c_bytes(&self) -> &[u8] {
        T::c_bytes(self)
    }
    fn stops_at_nul(&self) -> bool {
        T::stops_at_nul(self)
    }
    fn ignores_precision(&self) -> bool {
        T::ignores_precision(self)
    }
}

impl<T: CBytes + ?Sized> CBytes for &mut T {
    fn c_bytes(&self) -> &[u8] {
        T::c_bytes(self)
    }
    fn stops_at_nul(&self) -> bool {
        T::stops_at_nul(self)
    }
    fn ignores_precision(&self) -> bool {
        T::ignores_precision(self)
    }
}

macro_rules! c_bytes_pointer {
    ($ty:ident) => {
        impl<T: CBytes + ?Sized> CBytes for $ty<T> {
            fn c_bytes(&self) -> &[u8] {
                T::c_bytes(self)
            }
            fn stops_at_nul(&self) -> bool {
                T::stops_at_nul(self)
            }
            fn ignores_precision(&self) -> bool {
                T::ignores_precision(self)
            }
        }
    };
}
c_bytes_pointer!(Box);
c_bytes_pointer!(Rc);
c_bytes_pointer!(Arc);

impl<B> CBytes for Cow<'_, B>
where
    B: CBytes + ToOwned + ?Sized,
{
    fn c_bytes(&self) -> &[u8] {
        B::c_bytes(self)
    }
    fn stops_at_nul(&self) -> bool {
        B::stops_at_nul(self)
    }
    fn ignores_precision(&self) -> bool {
        B::ignores_precision(self)
    }
}

/// A destination for [`csnprintf!`].
///
/// Implementations may exclude a permanently reserved final NUL from the
/// returned region and return `true`. Plain byte slices return their whole
/// region and `false`, reserving one byte dynamically for termination.
pub trait CSnprintfDest {
    /// Returns the writable region and whether a NUL is guaranteed immediately
    /// after that region.
    fn dest(&mut self) -> (&mut [u8], bool);
}

impl CSnprintfDest for [u8] {
    fn dest(&mut self) -> (&mut [u8], bool) {
        (self, false)
    }
}

impl<const N: usize> CSnprintfDest for [u8; N] {
    fn dest(&mut self) -> (&mut [u8], bool) {
        (&mut self[..], false)
    }
}

impl<T: CSnprintfDest + ?Sized> CSnprintfDest for &mut T {
    fn dest(&mut self) -> (&mut [u8], bool) {
        T::dest(self)
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
