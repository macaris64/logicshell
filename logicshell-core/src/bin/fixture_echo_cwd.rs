// Test fixture: prints the current working directory.
fn main() {
    println!("{}", std::env::current_dir().unwrap().display());
}
