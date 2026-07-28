//! zstd-compressed file I/O. Files are single zstd frames; reads transparently
//! accept uncompressed content (magic-byte sniff) so plain files keep working.

use std::fs;
use std::io;
use std::path::Path;

const ZSTD_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];
const LEVEL: i32 = 3;

pub fn write_compressed(path: &Path, data: &[u8]) -> io::Result<()> {
    fs::write(path, zstd::encode_all(data, LEVEL)?)
}

/// Read a file, decompressing if it is a zstd frame, passing through otherwise.
pub fn read_text(path: &Path) -> io::Result<String> {
    let bytes = fs::read(path)?;
    let bytes = if bytes.starts_with(&ZSTD_MAGIC) {
        zstd::decode_all(&bytes[..])?
    } else {
        bytes
    };
    String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_passthrough() {
        let dir = std::env::temp_dir().join(format!("wltimer-zio-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let compressed = dir.join("a.zst");
        write_compressed(&compressed, "hello **world**".as_bytes()).unwrap();
        assert_ne!(fs::read(&compressed).unwrap(), b"hello **world**");
        assert_eq!(read_text(&compressed).unwrap(), "hello **world**");

        let plain = dir.join("b.md");
        fs::write(&plain, "plain text").unwrap();
        assert_eq!(read_text(&plain).unwrap(), "plain text");
        fs::remove_dir_all(&dir).unwrap();
    }
}
