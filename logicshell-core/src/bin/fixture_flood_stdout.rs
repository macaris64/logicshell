// Test fixture: writes N bytes of 'x' to stdout (N from argv[1], default 1024).
// Exits 0 even on broken pipe (reader closed early) so truncation tests are clean.
use std::io::Write;

fn main() {
    let n: usize = std::env::args()
        .nth(1)
        .as_deref()
        .unwrap_or("1024")
        .parse()
        .unwrap_or(1024);

    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    let chunk_size = 65_536_usize.min(n.max(1));
    let chunk = vec![b'x'; chunk_size];
    let mut remaining = n;
    while remaining > 0 {
        let to_write = remaining.min(chunk.len());
        if lock.write_all(&chunk[..to_write]).is_err() {
            // Broken pipe or other write error — reader closed early, exit cleanly.
            break;
        }
        remaining -= to_write;
    }
}
