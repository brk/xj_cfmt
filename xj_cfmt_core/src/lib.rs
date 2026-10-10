//! C printing syntax, independent of rendering and the host ABI.
#![no_std]
extern crate alloc;
use alloc::vec::Vec;
use core::fmt;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flags {
    pub minus: bool,
    pub plus: bool,
    pub space: bool,
    pub hash: bool,
    pub zero: bool,
    pub grouping: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Count {
    Absent,
    Constant(u32),
    Argument(usize),
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Length {
    #[default]
    None,
    Hh,
    H,
    L,
    Ll,
    J,
    Z,
    T,
    BigL,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Conversion {
    pub flags: Flags,
    pub width: Count,
    pub precision: Count,
    pub length: Length,
    pub conversion: u8,
    pub value_argument: Option<usize>,
    pub source_offset: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Piece<'a> {
    Literal(&'a [u8]),
    Conversion(Conversion),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Incomplete,
    UnknownConversion,
    InvalidLength,
    InvalidPercent,
    Positional,
    CountOverflow,
    Unsupported,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error {
    pub offset: usize,
    pub kind: ErrorKind,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "C format {:?} at byte {}", self.kind, self.offset)
    }
}
impl core::error::Error for Error {}

/// Bytes visible to a C string consumer.
pub fn c_bytes(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())]
}
pub struct Parser<'a> {
    bytes: &'a [u8],
    offset: usize,
    argument: usize,
    failed: bool,
}
pub fn parse(bytes: &[u8]) -> Parser<'_> {
    Parser {
        bytes: c_bytes(bytes),
        offset: 0,
        argument: 0,
        failed: false,
    }
}
impl Parser<'_> {
    fn arg(&mut self) -> usize {
        let a = self.argument;
        self.argument += 1;
        a
    }
    fn number(&mut self) -> Result<Option<u32>, Error> {
        let start = self.offset;
        let mut n = 0u32;
        while let Some(&b) = self.bytes.get(self.offset).filter(|b| b.is_ascii_digit()) {
            n = n
                .checked_mul(10)
                .and_then(|n| n.checked_add((b - b'0') as u32))
                .filter(|&n| n <= i32::MAX as u32)
                .ok_or(Error {
                    offset: start,
                    kind: ErrorKind::CountOverflow,
                })?;
            self.offset += 1;
        }
        if self.bytes.get(self.offset) == Some(&b'$') {
            return Err(Error {
                offset: self.offset,
                kind: ErrorKind::Positional,
            });
        }
        Ok((self.offset != start).then_some(n))
    }
    fn count(&mut self, default: Count) -> Result<Count, Error> {
        if self.bytes.get(self.offset) == Some(&b'*') {
            self.offset += 1;
            if self.number()?.is_some() {
                return Err(Error {
                    offset: self.offset,
                    kind: ErrorKind::Positional,
                });
            }
            Ok(Count::Argument(self.arg()))
        } else {
            Ok(self.number()?.map(Count::Constant).unwrap_or(default))
        }
    }
    fn conversion(&mut self, start: usize) -> Result<Conversion, Error> {
        let mut flags = Flags::default();
        while let Some(b) = self.bytes.get(self.offset) {
            match b {
                b'-' => flags.minus = true,
                b'+' => flags.plus = true,
                b' ' => flags.space = true,
                b'#' => flags.hash = true,
                b'0' => flags.zero = true,
                b'\'' => flags.grouping = true,
                _ => break,
            }
            self.offset += 1;
        }
        let width = self.count(Count::Absent)?;
        let precision = if self.bytes.get(self.offset) == Some(&b'.') {
            self.offset += 1;
            self.count(Count::Constant(0))?
        } else {
            Count::Absent
        };
        let length = match self.bytes.get(self.offset) {
            Some(b'h') => {
                self.offset += 1;
                if self.bytes.get(self.offset) == Some(&b'h') {
                    self.offset += 1;
                    Length::Hh
                } else {
                    Length::H
                }
            }
            Some(b'l') => {
                self.offset += 1;
                if self.bytes.get(self.offset) == Some(&b'l') {
                    self.offset += 1;
                    Length::Ll
                } else {
                    Length::L
                }
            }
            Some(b'j') => {
                self.offset += 1;
                Length::J
            }
            Some(b'z') => {
                self.offset += 1;
                Length::Z
            }
            Some(b't') => {
                self.offset += 1;
                Length::T
            }
            Some(b'L') => {
                self.offset += 1;
                Length::BigL
            }
            _ => Length::None,
        };
        let conversion = *self.bytes.get(self.offset).ok_or(Error {
            offset: start,
            kind: ErrorKind::Incomplete,
        })?;
        self.offset += 1;
        if !b"diouxXfFeEgGaAcspn%m".contains(&conversion) {
            return Err(Error {
                offset: self.offset - 1,
                kind: ErrorKind::UnknownConversion,
            });
        }
        let valid = match conversion {
            b'd' | b'i' | b'o' | b'u' | b'x' | b'X' | b'n' => length != Length::BigL,
            b'f' | b'F' | b'e' | b'E' | b'g' | b'G' | b'a' | b'A' => {
                matches!(length, Length::None | Length::L | Length::BigL)
            }
            b'c' | b's' => matches!(length, Length::None | Length::L),
            _ => length == Length::None,
        };
        if !valid {
            return Err(Error {
                offset: start,
                kind: ErrorKind::InvalidLength,
            });
        }
        if conversion == b'%'
            && (flags != Flags::default() || width != Count::Absent || precision != Count::Absent)
        {
            return Err(Error {
                offset: start,
                kind: ErrorKind::InvalidPercent,
            });
        }
        let value_argument = if b"%m".contains(&conversion) {
            None
        } else {
            Some(self.arg())
        };
        Ok(Conversion {
            flags,
            width,
            precision,
            length,
            conversion,
            value_argument,
            source_offset: start,
        })
    }
}
impl<'a> Iterator for Parser<'a> {
    type Item = Result<Piece<'a>, Error>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.offset == self.bytes.len() {
            return None;
        }
        let start = self.offset;
        if self.bytes[start] != b'%' {
            while self.offset < self.bytes.len() && self.bytes[self.offset] != b'%' {
                self.offset += 1;
            }
            return Some(Ok(Piece::Literal(&self.bytes[start..self.offset])));
        }
        self.offset += 1;
        let result = self.conversion(start).map(Piece::Conversion);
        self.failed = result.is_err();
        Some(result)
    }
}

/// C type required by va_arg, before rendering applies narrowing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Requirement {
    Int,
    UInt,
    Long,
    ULong,
    LongLong,
    ULongLong,
    IntMax,
    UIntMax,
    SignedSize,
    Size,
    PtrDiff,
    UnsignedPtrDiff,
    Double,
    WideChar,
    String,
    Pointer,
    Count(Length),
}
#[derive(Debug)]
pub struct Plan<'a> {
    pub pieces: Vec<Piece<'a>>,
    pub requirements: Vec<Requirement>,
    pub count_stores: bool,
}
impl<'a> Plan<'a> {
    /// Recognizes syntax and checks the initial narrow POSIX execution profile.
    pub fn new(bytes: &'a [u8]) -> Result<Self, Error> {
        let pieces = parse(bytes).collect::<Result<Vec<_>, _>>()?;
        let mut requirements = Vec::new();
        let mut count_stores = false;
        for piece in &pieces {
            let Piece::Conversion(c) = piece else {
                continue;
            };
            if c.flags.grouping
                || c.conversion == b'm'
                || c.length == Length::BigL
                || (c.conversion == b's' && c.length == Length::L)
                || (c.conversion == b'n'
                    && (c.flags != Flags::default()
                        || c.width != Count::Absent
                        || c.precision != Count::Absent))
            {
                return Err(Error {
                    offset: c.source_offset,
                    kind: ErrorKind::Unsupported,
                });
            }
            for count in [c.width, c.precision] {
                if let Count::Argument(index) = count {
                    debug_assert_eq!(index, requirements.len());
                    requirements.push(Requirement::Int);
                }
            }
            if let Some(index) = c.value_argument {
                debug_assert_eq!(index, requirements.len());
                use Requirement::*;
                let r = match c.conversion {
                    b'd' | b'i' => match c.length {
                        Length::L => Long,
                        Length::Ll => LongLong,
                        Length::J => IntMax,
                        Length::Z => SignedSize,
                        Length::T => PtrDiff,
                        _ => Int,
                    },
                    b'u' | b'o' | b'x' | b'X' => match c.length {
                        Length::L => ULong,
                        Length::Ll => ULongLong,
                        Length::J => UIntMax,
                        Length::Z => Size,
                        Length::T => UnsignedPtrDiff,
                        Length::H | Length::Hh => Int,
                        _ => UInt,
                    },
                    b'c' => {
                        if c.length == Length::L {
                            WideChar
                        } else {
                            Int
                        }
                    }
                    b's' => String,
                    b'p' => Pointer,
                    b'n' => {
                        count_stores = true;
                        Count(c.length)
                    }
                    _ => Double,
                };
                requirements.push(r);
            }
        }
        Ok(Self {
            pieces,
            requirements,
            count_stores,
        })
    }
}
