use flate2::bufread::GzDecoder;
use sha2::{Digest, Sha256};
use std::io::Read;

pub(crate) const MAGIC: &[u8; 8] = b"OBSGZIP1";
const HEADER_BYTES: usize = 48;

/// Decode one bounded gzip member. No writes or install side effects.
pub(crate) fn decode(bytes: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    if bytes.len() < HEADER_BYTES || bytes.len() > limit || &bytes[..8] != MAGIC {
        return Err("compressed payload header or size is invalid".into());
    }
    let expected = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
    if expected < 16 || expected > limit as u64 {
        return Err("compressed payload expanded size exceeds its bound".into());
    }
    let mut decoder = GzDecoder::new(&bytes[HEADER_BYTES..]);
    let mut raw = Vec::new();
    // Read one extra byte to detect oversized output; never trust gzip ISIZE.
    decoder
        .by_ref()
        .take(expected + 1)
        .read_to_end(&mut raw)
        .map_err(|e| format!("compressed payload is corrupt: {e}"))?;
    if raw.len() as u64 != expected {
        return Err("compressed payload expanded size mismatch".into());
    }
    if !decoder.get_ref().is_empty() {
        return Err("compressed payload contains trailing data".into());
    }
    if Sha256::digest(&raw)[..] != bytes[16..48] {
        return Err("compressed payload hash mismatch".into());
    }
    // Reject nesting before the caller invokes the existing inner parser.
    if &raw[..8] != b"OBSMACH1" {
        return Err("compressed payload must contain an OBSMACH1 package".into());
    }
    Ok(raw)
}

#[cfg(test)]
pub(crate) fn pack_fixture(raw: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut result = MAGIC.to_vec();
    result.extend_from_slice(&(raw.len() as u64).to_le_bytes());
    result.extend_from_slice(&Sha256::digest(raw));
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    gzip.write_all(raw).unwrap();
    result.extend_from_slice(&gzip.finish().unwrap());
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    const RAW: &[u8] = b"OBSMACH1test payload contents repeated repeated repeated";
    #[test]
    fn round_trip() {
        assert_eq!(decode(&pack_fixture(RAW), 1024).unwrap(), RAW);
    }
    #[test]
    fn rejects_bad_hash_crc_and_truncation() {
        let packed = pack_fixture(RAW);
        for index in [16, packed.len() - 8] {
            let mut bad = packed.clone();
            bad[index] ^= 1;
            assert!(decode(&bad, 1024).is_err());
        }
        for end in 0..packed.len() {
            assert!(decode(&packed[..end], 1024).is_err());
        }
    }
    #[test]
    fn rejects_trailing_bytes_and_multiple_members() {
        let mut packed = pack_fixture(RAW);
        packed.push(0);
        assert!(decode(&packed, 1024).is_err());
        let mut packed = pack_fixture(RAW);
        packed.extend_from_slice(&pack_fixture(RAW)[HEADER_BYTES..]);
        assert!(decode(&packed, 1024).is_err());
    }
    #[test]
    fn rejects_size_lies_and_bombs() {
        for declared in [
            0u64,
            15,
            16,
            RAW.len() as u64 - 1,
            RAW.len() as u64 + 1,
            1025,
            u64::MAX,
        ] {
            let mut packed = pack_fixture(RAW);
            packed[8..16].copy_from_slice(&declared.to_le_bytes());
            assert!(decode(&packed, 1024).is_err());
        }
        let mut raw = b"OBSMACH1".to_vec();
        raw.resize(100_000, 0);
        let mut packed = pack_fixture(&raw);
        packed[8..16].copy_from_slice(&32u64.to_le_bytes());
        assert!(decode(&packed, 1024).is_err());
    }
    #[test]
    fn rejects_nested_packages() {
        assert!(decode(&pack_fixture(&pack_fixture(RAW)), 1024).is_err());
    }
}
