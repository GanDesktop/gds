//! Bounded gzip/DEFLATE decompression (`.svgz`) and IEEE CRC-32 helpers.
//!
//! `usvg` delegates `.svgz` decompression to an unbounded in-memory gunzip call; pre-decompressing
//! with [`decompress_svgz_bounded`] caps the decoded XML size so malicious compressed SVGs fail
//! deterministically with [`Error::TooLarge`] instead of exhausting host memory.

use std::io::Read;

use flate2::bufread::MultiGzDecoder;

use super::Error;

/// Computes the IEEE 802.3 CRC-32 checksum over `bytes`.
pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    crc32fast::hash(bytes)
}

/// Returns `true` when `bytes` begins with the RFC 1952 gzip magic header (`1F 8B`).
pub(crate) fn is_gzip_magic(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x1F, 0x8B])
}

/// Decompresses a gzip-wrapped `.svgz` byte stream, enforcing a strict output cap of `max_bytes`.
///
/// # Errors
/// Returns [`Error::TooLarge`] when the uncompressed XML exceeds `max_bytes`, or an SVG parse error
/// when the gzip container or DEFLATE stream is malformed.
pub(crate) fn decompress_svgz_bounded(data: &[u8], max_bytes: usize) -> Result<Vec<u8>, Error> {
    if !is_gzip_magic(data) {
        return Err(malformed_gzip());
    }
    let limit = u64::try_from(max_bytes)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let mut output = Vec::new();
    let mut reader = MultiGzDecoder::new(data).take(limit);
    reader
        .read_to_end(&mut output)
        .map_err(|_| malformed_gzip())?;

    if output.len() > max_bytes {
        return Err(Error::TooLarge {
            limit_bytes: max_bytes as u64,
            actual_bytes: output.len() as u64,
        });
    }
    Ok(output)
}

fn malformed_gzip() -> Error {
    Error::Svg(resvg::usvg::Error::MalformedGZip)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::{Compression, write::GzEncoder};

    use super::*;

    fn gzip_member(payload: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(payload).expect("write gzip payload");
        encoder.finish().expect("finish gzip member")
    }

    #[test]
    fn crc32_matches_ieee_check_vector() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn multi_member_exact_limit_overflow_bad_crc_and_trailing_garbage() {
        let mut two_members = gzip_member(b"abc");
        two_members.extend_from_slice(&gzip_member(b"def"));

        // Exact limit reads through the trailer and succeeds.
        assert_eq!(
            decompress_svgz_bounded(&two_members, 6).expect("exact limit"),
            b"abcdef"
        );

        // One byte over the limit reports TooLarge with the first excess byte observed.
        let err = decompress_svgz_bounded(&two_members, 5).expect_err("limit + 1 must fail");
        assert!(matches!(
            err,
            Error::TooLarge {
                limit_bytes: 5,
                actual_bytes: 6,
            }
        ));

        // Corrupted trailer CRC fails even when output length equals max_bytes.
        let mut bad_crc = gzip_member(b"abcdef");
        let crc_pos = bad_crc.len() - 8;
        bad_crc[crc_pos] ^= 0xFF;
        assert!(matches!(
            decompress_svgz_bounded(&bad_crc, 6),
            Err(Error::Svg(resvg::usvg::Error::MalformedGZip))
        ));

        // Non-gzip trailing suffix fails even when output length equals max_bytes.
        let mut trailing_garbage = gzip_member(b"abcdef");
        trailing_garbage.extend_from_slice(b"junk");
        assert!(matches!(
            decompress_svgz_bounded(&trailing_garbage, 6),
            Err(Error::Svg(resvg::usvg::Error::MalformedGZip))
        ));
    }
}
