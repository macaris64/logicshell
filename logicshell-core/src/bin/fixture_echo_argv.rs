// Test fixture: prints each argv element (after argv[0]) on its own line.
fn main() {
    for arg in std::env::args().skip(1) {
        println!("{arg}");
    }
}
