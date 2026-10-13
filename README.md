# xj_cfmt

`xj_cfmt` implements byte-oriented C printing and provides separate macros for
Rust brace syntax. C literals and runtime formats use the same parser and
renderer, regardless of the output destination.

```rust
let mut output = Vec::new();
assert_eq!(xj_cfmt::sprintf!(output, b"%*.*s:%hhd", -5, 2, b"abcdef", 255), 8);
assert_eq!(output, b"ab   :-1");

let mut buffer = [0u8; 4];
assert_eq!(xj_cfmt::snprintf!(buffer, 4, c"%s", c"abcdef"), 6);
assert_eq!(&buffer, b"abc\0");
```

The workspace contains:

- `xj_cfmt_core`: a `no_std` printf parser, borrowed literal spans, conversion
  descriptors, sequential argument requirements, and supported-profile analysis.
  It depends on `xj_bytes`, with no dependency on an ABI, libc, I/O, or rendering.
- `xj_cfmt`: C conversion execution, type-driven argument adaptation, safe and
  unsafe APIs, and non-stream destinations. `cformat` re-exports the core crate.
- `xj_cfmt_macros`: brace-format macros and validation of safe C literals.

`xj_cio` supplies stream adapters and the same C-format macro names.
The standalone `xj_bytes` crate supplies shared `ByteView` implementations,
`c_bytes`, `to_c_str`, `CStrDest`, `CStrBuffer`, and `CStrOutput` storage policies.
It is `no_std` with `alloc`; its optional `std` feature adds Unix OS/path views.
Both `xj_cfmt` and `xj_cfmt_core` depend on it.
`xj_zstr` implements the shared destination trait without a feature flag.
Its `cfmt` feature supplies formatting argument adapters.

## C printing profile

The native POSIX profile supports `d i u o x X c s p n f F e E g G a A`,
`%%`, all ordinary flags, signed dynamic width and precision, and integer/count
lengths `hh h l ll j z t`. Floating `%l` means double. `%lc` uses native
`wint_t` conversion and the active C character locale; wide `%ls` is explicitly
unsupported. Floating conversions and `%p` use native libc to preserve target
rounding, spelling, padding, and locale behavior. Integers and narrow strings
are rendered as bytes without a Unicode conversion. Raw strings are read only
at their consuming conversion and precision bounds the read. `%n` stores occur
in conversion order and use the destination type named by the length modifier.
The profile assumes POSIX 32-bit int, at most 64-bit integer types, and 32-bit
wint_t; it does not silently substitute the translator host's ABI.

Positional arguments, locale grouping, `%m`, `long double`, and wide strings
return structured errors. Syntax recognition retains unsupported length and
conversion information. Unknown conversions, missing arguments, and incompatible
carriers also fail explicitly. Public C APIs return `-1` and set `EINVAL` for
these errors, `EOVERFLOW` for count overflow, the native errno for native
conversion failures, and the output error for destination failures. This is the
library's compatibility policy for inputs outside its profile; it is not a claim
that C defines behavior for invalid variadic calls. A null `%s` prints `(null)`;
a null `%n` destination is rejected.

`sprintf_raw!` and `snprintf_raw!` take raw destination pointers and require an `unsafe`
caller. Raw argument pointers are carried without format-dependent classification.
The caller must guarantee readable string storage and correctly aligned writable
count storage for the conversions that actually consume each pointer. The macros
never hide dereferences of caller-supplied raw pointers inside an unsafe block.

`sprintf!` and `snprintf!` accept bounded storage or growable `Vec<u8>`
destinations and safe representations: scalars, C string views, and borrowed bytes. Their argument
trait rejects raw pointers. Literal `%n` is rejected at compilation; runtime `%n`
returns an error. Guided byte carriers support `%s`; using them for `%p`, `%n`,
or numeric conversions is an error, so no unrelated pointer identity is invented.
`sprintf_unchecked!` and `snprintf_unchecked!` combine safe storage with raw
arguments and require an unsafe caller. All arguments, including unused extras,
are evaluated once in their source order. A raw runtime format requires an
explicit `CStr::from_ptr` at the call site.

All C calls buffer the whole result before transferring it to the destination.
Bounded storage applies C truncation and termination rules; the return counts
bytes before truncation and excludes the terminator. A direct `Vec<u8>` destination
replaces its contents and grows as needed, without storing a terminator.
An explicit `snprintf` size limits its payload
to `size - 1` bytes; size zero leaves it unchanged. Pass `vector.as_mut_slice()`
for a bounded buffer using the initialized length, preserving its length and
bytes beyond the written terminator. Raw unbounded destinations never create
an oversized mutable slice. Incremental stream output, bounded rendering without
a full-result allocation, and compile-time plans are deferred optimizations.

## Brace syntax

The existing `cprintf!`, `ceprintf!`, `cemit!`, `cwrite!`, and `csnprintf!` macros
accept Rust format syntax. Display placeholders use `ByteDisplay` when available,
with byte-oriented width and precision. `ByteView::view_bytes` supplies the
complete available bytes; `ByteDisplay::byte_format` selects C string, literal
byte, or character handling. `RawBytes(&value)` selects literal byte handling,
including stored NULs. C printing `%s` always applies C string semantics.
Other placeholders delegate to standard
Rust formatting traits. These are useful for hand-written Rust; the translator
emits C-format macros that retain the original C syntax.

## Bounded string destinations

`CStrBuffer::cstr_dest()` produces the same `CStrDest` view for arrays, slices,
`ZStr`, and `ZString`. For a vector, use `vector.as_mut_slice().cstr_dest()`
to select its initialized bytes as bounded storage. `CStrDest::limit(n)` selects
a C size including space for the terminating NUL, with zero meaning no writes.
Limits beyond the view's extent are rejected. String writers use `payload_mut()`,
`terminate(len)`, or `write_truncated(bytes)`; nonterminating writers can use
`region_mut()`, which excludes a string buffer's protected final NUL.

For example, `csnprintf!(buffer.cstr_dest().limit(n), "{}", value)` uses Rust
formatting within C size `n`; import `xj_cfmt::CStrBuffer` for the method. A
`ZStr` also provides the convenience `buffer.limit(n)`. A smaller boundary need
not already contain a NUL. Both printing engines share the destination view's
size and termination rules.

## Development

The translator depends on `xj_bytes` and `xj_cfmt_core`. Their local crates.io
overrides and lockfile support development with Tenjin's locked builds. Publish
bytes, core, macros, cfmt, zstr, then cio, and replace development overrides with published pins
before releasing the translator. The test suites compare C literal and runtime
printing with libc and exercise safe API compilation failures.
