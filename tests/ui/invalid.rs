use xj_cfmt::{Byte, cwrite};

struct Neither;

fn main() {
    let mut output = Vec::new();
    let runtime = "{}";
    let _ = cwrite!(&mut output, runtime, 1);
    let _ = cwrite!(&mut output, "{}", 1, 2);
    let _ = cwrite!(&mut output, "{}");
    let _ = cwrite!(&mut output, "{:q}", 1);
    let _ = cwrite!(&mut output, "{:x}", c"text");
    let _ = cwrite!(&mut output, "{:x}", Byte(1));
    let _ = cwrite!(&mut output, "{}", Neither);
    let _ = cwrite!(&mut output, "{:é>5}", "x");
    let _ = cwrite!(&mut output, b"{:\xc3\xa9>5}", b"x");
}
