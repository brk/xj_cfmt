# xj_cfmt

`xj_cfmt` provides `printf`-family macros for C-to-Rust translations. It uses
ordinary Rust format syntax, but formats string-like values as raw bytes so
width and precision match C even for non-UTF-8 and multibyte data.

```rust
use xj_cfmt::{Byte, csnprintf, cwrite};

let mut output = Vec::new();
assert_eq!(cwrite!(&mut output, "{:>5}|{}", "é", Byte(0xff)), 7);
assert_eq!(output, b"   \xc3\xa9|\xff");

let mut buffer = [0; 4];
assert_eq!(csnprintf!(&mut buffer, "{}", b"abcdef"), 6);
assert_eq!(buffer, *b"abc\0");
```

The crates are:

- `xj_cfmt`: public traits, macros, sinks, and runtime.
- `xj_cfmt_macros`: the proc-macro parser and code generator.

Display (`{}`) placeholders use the byte path when their argument implements
`CBytes`; otherwise they use `Display`. Explicit formatting types such as
`{:x}`, `{:?}`, and `{:p}` always use the corresponding standard formatting
trait. Fill characters must be ASCII. Both string and byte-string format
literals are accepted.

`cprintf!`, `ceprintf!`, and `cemit!` render each call before invoking its sink,
so a sink receives exactly one `emit` call. The default standard-stream backend
uses C `fwrite`, preserving ordering and buffering when translated code still
uses libc stdio. Enable `std-stdio` to use Rust's locked stdout and stderr.

## Translator mapping

| C | Emitted Rust |
| --- | --- |
| `printf("%-10s: %5d\n", name, n)` | `cprintf!("{:<10}: {:5}\n", name, n)` |
| `printf("%10s", s)` | `cprintf!("{:>10}", s)` |
| `printf("%c", c)` | `cprintf!("{}", Byte(c as u8))` |
| `printf("%hhu", x)` | `cprintf!("{}", x as u8)` |
| `printf("%.*s", n, buf)` | `cprintf!("{:.*}", n as usize, buf)` |
| `printf("%*d", w, x)` | `cprintf!("{:>1$}", x, w as usize)` |
| `printf("\xff%s", s)` | `cprintf!(b"\xff{}", s)` |
| `fprintf(stderr, "error: %s\n", m)` | `ceprintf!("error: {}\n", m)` |
| `fprintf(f, "%s\n", s)` with `xj_cio` | `xj_cio::cfprintf!(f, "{}\n", s)` |
| `snprintf(buf, sizeof buf, "%08x", v)` | `csnprintf!(&mut buf, "{:08x}", v)` |
| `snprintf(out, n, "%s", s)` for a slice | `csnprintf!(&mut out[..n], "{}", s)` |
| `snprintf(out, n, "%s", s)` for `ZStr` | `csnprintf!(out.limit(n), "{}", s)` |

Dynamic width and precision arguments are `usize`. Negative C `*` counts,
runtime format strings, wide strings, `%n`, and locale-dependent formatting are
outside this crate's scope.
