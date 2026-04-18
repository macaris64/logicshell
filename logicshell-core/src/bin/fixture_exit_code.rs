// Test fixture: exits with the code supplied as argv[1] (default 0).
fn main() {
    let code: i32 = std::env::args()
        .nth(1)
        .as_deref()
        .unwrap_or("0")
        .parse()
        .unwrap_or(0);
    std::process::exit(code);
}
