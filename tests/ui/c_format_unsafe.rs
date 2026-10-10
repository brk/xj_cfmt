fn main() {
    let mut out = [0u8; 10];
    xj_cfmt::sprintf!(out.as_mut_ptr(), b"%s", c"hello".as_ptr());
}
