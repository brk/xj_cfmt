fn main() {
    let mut out = Vec::new();
    xj_cfmt::sprintf_safe!(out, b"%s", c"hello".as_ptr());
}
