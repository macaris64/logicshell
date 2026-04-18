// Test fixture: prints the value of the env var named by argv[1].
fn main() {
    let var = std::env::args().nth(1).unwrap_or_default();
    println!("{}", std::env::var(&var).unwrap_or_default());
}
