use hex::encode;
use std::io::Read;

pub fn new(prefix: &str) -> String {
    format!("{prefix}_{}", encode(random_bytes::<8>()))
}

pub fn session() -> String {
    encode(random_bytes::<16>())
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    std::fs::File::open("/dev/urandom")
        .expect("open /dev/urandom")
        .read_exact(&mut buf)
        .expect("read /dev/urandom");
    buf
}
