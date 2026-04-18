// Test fixture: copies stdin to stdout verbatim.
use std::io::{self, Read, Write};

fn main() {
    let mut buf = Vec::new();
    io::stdin().read_to_end(&mut buf).unwrap();
    io::stdout().write_all(&buf).unwrap();
}
