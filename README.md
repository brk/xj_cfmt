# xj_cfmt

`xj_cfmt` implements byte-oriented C printing and provides separate macros for
Rust brace syntax. C literals and runtime formats use the same parser and
renderer, regardless of the output destination.

```rust
let mut output = Vec::new();
assert_eq!(xj_cfmt::sprintf_safe!(output, b"%*.*s:%hhd", -5, 2, b"abcdef", 255), 8);
assert_eq!(output, b"ab   :-1");

let mut buffer = [0u8; 4];
assert_eq!(xj_cfmt::snprintf_safe!(buffer, 4, c"%s", c"abcdef"), 6);
assert_eq!(&buffer, b"abc\0");
```

The workspace contains:

- `xj_cfmt_core`: a `no_std` byte parser, borrowed literal spans, conversion
  descriptors, sequential argument requirements, and supported-profile analysis.
  It has no dependency on an ABI, libc, I/O, or other xj crates.
- `xj_cfmt`: C conversion execution, type-driven argument adaptation, safe and
  unsafe APIs, and non-stream destinations. `cformat` re-exports the core crate.
- `xj_cfmt_macros`: brace-format macros and validation of safe C literals.

`xj_cio` supplies stream adapters and the same C-format macro names.
`xj_zstr` implements argument and bounded-destination traits behind `cfmt`.

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

`sprintf!` and `snprintf!` take raw destination pointers and require an `unsafe`
caller. Raw argument pointers are carried without format-dependent classification.
The caller must guarantee readable string storage and correctly aligned writable
count storage for the conversions that actually consume each pointer. The macros
never hide dereferences of caller-supplied raw pointers inside an unsafe block.

`sprintf_safe!` and `snprintf_safe!` accept bounded storage or a `Vec<u8>` and
safe representations: scalars, C string views, and borrowed bytes. Their argument
trait rejects raw pointers. Literal `%n` is rejected at compilation; runtime `%n`
returns an error. Guided byte carriers support `%s`; using them for `%p`, `%n`,
or numeric conversions is an error, so no unrelated pointer identity is invented.
`sprintf_unchecked!` and `snprintf_unchecked!` combine safe storage with raw
arguments and require an unsafe caller. All arguments, including unused extras,
are evaluated once in their source order. A raw runtime format requires an
explicit `CStr::from_ptr` at the call site.

All C calls buffer the whole result before transferring it to the destination.
Bounded storage applies C truncation and termination rules; the return counts
bytes before truncation and excludes the terminator. `Vec<u8>` holds string
content without a stored terminator. Raw unbounded destinations never create an
oversized mutable slice. Incremental stream output, bounded rendering without a
full-result allocation, and compile-time plans are deferred optimizations.

## Brace syntax

The existing `cprintf!`, `ceprintf!`, `cemit!`, `cwrite!`, and `csnprintf!` macros
accept Rust format syntax. Display placeholders use `CBytes` when available,
with byte-oriented width and precision. Other placeholders delegate to standard
Rust formatting traits. These are useful for hand-written Rust; the translator
emits C-format macros that retain the original C syntax.

## Development

The translator depends only on `xj_cfmt_core`. Its local crates.io override and
lockfile support development with Tenjin's locked builds. Publish core, macros,
cfmt, zstr, then cio, and replace development overrides with published pins
before releasing the translator. The test suites compare C literal and runtime
printing with libc and exercise safe API compilation failures.
