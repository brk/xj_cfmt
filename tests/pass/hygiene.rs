#![forbid(unsafe_code)]
#![deny(warnings)]

use xj_cfmt as renamed;

macro_rules! wrapped {
    ($($tokens:tt)*) => { renamed::cwrite!($($tokens)*) };
}

fn main() {
    let __w = 1;
    let __a0 = 2;
    let w = 3;
    let v = 4;
    #[allow(non_snake_case)]
    let Arg = 5;
    #[allow(non_snake_case)]
    let Spec = 6;
    let captured = 7;
    let mut output = Vec::new();
    let count = wrapped!(
        &mut output,
        "{} {} {} {} {} {} {captured} {named}",
        __w,
        __a0,
        w,
        v,
        Arg,
        Spec,
        named = 8,
    );
    assert_eq!(count, 15);
    assert_eq!(output, b"1 2 3 4 5 6 7 8");
}
