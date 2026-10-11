use crate::{ByteDisplay, ByteFormat, CSink, CStrBuffer, CStrDest};
use libc::c_int;
use std::fmt;
use std::io::{self, Write};

pub use xj_cfmt_macros::{cemit_impl, ceprintf_impl, cprintf_impl, csnprintf_impl, cwrite_impl};

const STACK_CAPACITY: usize = 512;
const C_INT_OVERFLOW: usize = c_int::MAX as usize + 1;

/// Alignment parsed from a byte-oriented placeholder.
#[derive(Clone, Copy)]
pub enum Align {
    /// Use the byte path's default (left).
    Unspecified,
    /// Pad after the value.
    Left,
    /// Pad before the value.
    Right,
    /// Divide padding around the value.
    Center,
}

/// Runtime fields needed by byte-oriented formatting.
pub struct Spec {
    /// ASCII fill byte.
    pub fill: u8,
    /// Requested alignment.
    pub align: Align,
    /// Minimum width in bytes.
    pub width: Option<usize>,
    /// Maximum byte count.
    pub precision: Option<usize>,
}

/// Wrapper on which Display dispatch is probed.
pub struct Arg<'a, T: ?Sized>(pub &'a T);

/// Erased standard-Display formatting callback.
pub type StdFmt<'a> = &'a mut dyn FnMut(&mut dyn Write, &dyn fmt::Display) -> io::Result<()>;

/// Preferred byte-oriented Display route.
#[diagnostic::on_unimplemented(message = "this value implements neither ByteDisplay nor Display")]
pub trait ViaBytes {
    /// Formats the wrapped argument.
    fn fmt_display(&self, writer: &mut dyn Write, spec: &Spec, std: StdFmt<'_>) -> io::Result<()>;
}

/// Fallback standard Display route.
#[diagnostic::on_unimplemented(message = "this value implements neither ByteDisplay nor Display")]
pub trait ViaStd {
    /// Formats the wrapped argument.
    fn fmt_display(&self, writer: &mut dyn Write, spec: &Spec, std: StdFmt<'_>) -> io::Result<()>;
}

impl<T: ByteDisplay + ?Sized> ViaBytes for &Arg<'_, T> {
    fn fmt_display(&self, writer: &mut dyn Write, spec: &Spec, _std: StdFmt<'_>) -> io::Result<()> {
        write_bytes(writer, self.0, spec)
    }
}

impl<T: fmt::Display + ?Sized> ViaStd for Arg<'_, T> {
    fn fmt_display(&self, writer: &mut dyn Write, _spec: &Spec, std: StdFmt<'_>) -> io::Result<()> {
        std(writer, &self.0)
    }
}

fn write_bytes<T: ByteDisplay + ?Sized>(
    writer: &mut dyn Write,
    value: &T,
    spec: &Spec,
) -> io::Result<()> {
    let mode = value.byte_format();
    let mut bytes = value.view_bytes();
    if let Some(precision) = spec.precision.filter(|_| mode != ByteFormat::Character) {
        bytes = &bytes[..bytes.len().min(precision)];
    }
    if mode == ByteFormat::CString {
        if let Some(end) = bytes.iter().position(|&byte| byte == 0) {
            bytes = &bytes[..end];
        }
    }
    let pad = spec
        .width
        .map_or(0, |width| width.saturating_sub(bytes.len()));
    let (before, after) = match spec.align {
        Align::Unspecified | Align::Left => (0, pad),
        Align::Right => (pad, 0),
        Align::Center => {
            let before = pad / 2;
            (before, pad - before)
        }
    };
    write_fill(writer, spec.fill, before)?;
    writer.write_all(bytes)?;
    write_fill(writer, spec.fill, after)
}

fn write_fill(writer: &mut dyn Write, fill: u8, mut count: usize) -> io::Result<()> {
    let block = [fill; 64];
    while count >= block.len() {
        writer.write_all(&block)?;
        count -= block.len();
    }
    writer.write_all(&block[..count])
}

/// Delegates standard formatting without `io::Write::write_fmt`'s panic on a
/// bare `fmt::Error`.
pub fn write_fmt(writer: &mut dyn Write, arguments: fmt::Arguments<'_>) -> io::Result<()> {
    struct Adapter<'a> {
        writer: &'a mut dyn Write,
        error: Option<io::Error>,
    }
    impl fmt::Write for Adapter<'_> {
        fn write_str(&mut self, text: &str) -> fmt::Result {
            match self.writer.write_all(text.as_bytes()) {
                Ok(()) => Ok(()),
                Err(error) => {
                    self.error = Some(error);
                    Err(fmt::Error)
                }
            }
        }
    }

    let mut adapter = Adapter {
        writer,
        error: None,
    };
    match fmt::write(&mut adapter, arguments) {
        Ok(()) => Ok(()),
        Err(_) => Err(adapter.error.unwrap_or_else(|| io::ErrorKind::Other.into())),
    }
}

/// Preserves the formatting trait implemented by a saved argument expression.
pub struct FmtRef<'a, T: ?Sized>(pub &'a T);

macro_rules! forward_fmt {
    ($trait:ident) => {
        impl<T: fmt::$trait + ?Sized> fmt::$trait for FmtRef<'_, T> {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::$trait::fmt(self.0, formatter)
            }
        }
    };
}
forward_fmt!(Debug);
forward_fmt!(LowerHex);
forward_fmt!(UpperHex);
forward_fmt!(Octal);
forward_fmt!(Binary);
forward_fmt!(LowerExp);
forward_fmt!(UpperExp);
forward_fmt!(Pointer);

/// Copies and type-checks a dynamic width or precision argument.
pub fn count(value: &usize) -> usize {
    *value
}

struct RenderBuffer {
    stack: [u8; STACK_CAPACITY],
    len: usize,
    heap: Option<Vec<u8>>,
}

impl RenderBuffer {
    fn new() -> Self {
        Self {
            stack: [0; STACK_CAPACITY],
            len: 0,
            heap: None,
        }
    }

    fn as_slice(&self) -> &[u8] {
        self.heap.as_deref().unwrap_or(&self.stack[..self.len])
    }
}

impl Write for RenderBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        if let Some(heap) = &mut self.heap {
            heap.try_reserve(bytes.len())
                .map_err(|_| io::ErrorKind::OutOfMemory)?;
            heap.extend_from_slice(bytes);
            return Ok(bytes.len());
        }
        if bytes.len() <= STACK_CAPACITY - self.len {
            self.stack[self.len..self.len + bytes.len()].copy_from_slice(bytes);
            self.len += bytes.len();
            return Ok(bytes.len());
        }
        let needed = self
            .len
            .checked_add(bytes.len())
            .ok_or(io::ErrorKind::OutOfMemory)?;
        let mut heap = Vec::new();
        heap.try_reserve(needed)
            .map_err(|_| io::ErrorKind::OutOfMemory)?;
        heap.extend_from_slice(&self.stack[..self.len]);
        heap.extend_from_slice(bytes);
        self.heap = Some(heap);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Renders into the stack-first buffer and invokes a sink exactly once.
pub fn emit<S, F>(sink: S, render: F) -> c_int
where
    S: CSink,
    F: FnOnce(&mut dyn Write) -> io::Result<()>,
{
    let mut buffer = RenderBuffer::new();
    let rendered = render(&mut buffer);
    let count = buffer.as_slice().len();
    let emitted = sink.emit(buffer.as_slice());
    if rendered.is_err() || count >= C_INT_OVERFLOW || emitted.is_err() {
        -1
    } else {
        count as c_int
    }
}

struct CountingWriter<'a, W: ?Sized> {
    writer: &'a mut W,
    count: usize,
}

impl<W: Write + ?Sized> Write for CountingWriter<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let written = self.writer.write(bytes)?;
        self.count = self.count.saturating_add(written).min(C_INT_OVERFLOW);
        Ok(written)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

/// Streams a call into its writer and maps its result to C's return convention.
pub fn write<W, F>(writer: &mut W, render: F) -> c_int
where
    W: Write + ?Sized,
    F: FnOnce(&mut dyn Write) -> io::Result<()>,
{
    let mut writer = CountingWriter { writer, count: 0 };
    let result = render(&mut writer);
    if result.is_err() || writer.count >= C_INT_OVERFLOW {
        -1
    } else {
        writer.count as c_int
    }
}

/// Method-call adapter that gives `cwrite!` the same receiver autoref behavior
/// as `std::write!`.
pub trait WriteExt: Write {
    /// Runs one generated formatting closure.
    fn __xj_cfmt_write<F>(&mut self, render: F) -> c_int
    where
        F: FnOnce(&mut dyn Write) -> io::Result<()>,
    {
        write(self, render)
    }
}

impl<W: Write + ?Sized> WriteExt for W {}

struct SnprintfWriter<'a> {
    destination: CStrDest<'a>,
    written: usize,
    count: usize,
}

impl Write for SnprintfWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let remaining = &mut self.destination.payload_mut()[self.written..];
        let copied = remaining.len().min(bytes.len());
        remaining[..copied].copy_from_slice(&bytes[..copied]);
        self.written += copied;
        self.count = self.count.saturating_add(bytes.len()).min(C_INT_OVERFLOW);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Applies C `snprintf` truncation and termination rules.
pub fn snprintf<D, F>(destination: &mut D, render: F) -> c_int
where
    D: CStrBuffer + ?Sized,
    F: FnOnce(&mut dyn Write) -> io::Result<()>,
{
    snprintf_dest(destination.cstr_dest(), render)
}

/// Applies `snprintf` behavior to an already-resolved destination view.
pub fn snprintf_dest<F>(destination: CStrDest<'_>, render: F) -> c_int
where
    F: FnOnce(&mut dyn Write) -> io::Result<()>,
{
    let mut writer = SnprintfWriter {
        destination,
        written: 0,
        count: 0,
    };
    let result = render(&mut writer);
    writer.destination.terminate(writer.written);
    if result.is_err() || writer.count >= C_INT_OVERFLOW {
        -1
    } else {
        writer.count as c_int
    }
}

#[cfg(not(feature = "std-stdio"))]
pub enum CStream {
    Stdout,
    Stderr,
}

#[cfg(all(
    not(feature = "std-stdio"),
    any(target_os = "linux", target_os = "android")
))]
unsafe extern "C" {
    static mut stdout: *mut libc::FILE;
    static mut stderr: *mut libc::FILE;
}

#[cfg(all(
    not(feature = "std-stdio"),
    any(target_os = "macos", target_os = "freebsd")
))]
unsafe extern "C" {
    static mut __stdoutp: *mut libc::FILE;
    static mut __stderrp: *mut libc::FILE;
}

#[cfg(all(
    not(feature = "std-stdio"),
    not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "freebsd"
    ))
))]
compile_error!(
    "xj_cfmt's C stdio backend supports Linux, Android, macOS, and FreeBSD; enable std-stdio elsewhere"
);

#[cfg(not(feature = "std-stdio"))]
pub fn emit_c_stdio(bytes: &[u8], stream: CStream) -> io::Result<()> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let file = unsafe {
        match stream {
            CStream::Stdout => stdout,
            CStream::Stderr => stderr,
        }
    };
    #[cfg(any(target_os = "macos", target_os = "freebsd"))]
    let file = unsafe {
        match stream {
            CStream::Stdout => __stdoutp,
            CStream::Stderr => __stderrp,
        }
    };

    let written = unsafe { libc::fwrite(bytes.as_ptr().cast(), 1, bytes.len(), file) };
    if written == bytes.len() {
        Ok(())
    } else {
        Err(io::ErrorKind::Other.into())
    }
}

pub use xj_cfmt_macros::validate_safe_format;
