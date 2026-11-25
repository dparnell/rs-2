//! Rust with std: threads, files, the environment.

fn main() {
    let sum: u64 = std::thread::spawn(|| (1..=100).sum()).join().unwrap();
    let files = std::fs::read_dir("C:\\").map_or(0, |d| d.count());
    println!("Hello from Rust, {}-bit: {sum}, {files} files in C:\\", usize::BITS);
}
