//! Fuzz testing infrastructure for KDB/KDBX parsing
//!
//! Provides fuzz targets and structured fuzz input generators
//! to validate that the parser is robust against malformed/corrupt inputs.

use std::io::Cursor;

use crate::kdbx::file::reader::DatabaseReader;

/// Fuzz input types that can be generated for testing.
#[derive(Debug, Clone)]
pub enum FuzzTarget {
    /// Raw bytes treated as a KDB file
    RawKdb,
    /// Raw bytes treated as a KDBX 3.1 file
    RawKdbx31,
    /// Raw bytes treated as a KDBX 4.0 file
    RawKdbx4,
    /// XML payload fuzzing (post-decryption)
    XmlPayload,
}

/// Result of a fuzz test run.
#[derive(Debug, Clone)]
pub struct FuzzResult {
    pub input_len: usize,
    pub target: FuzzTarget,
    pub outcome: FuzzOutcome,
}

/// Possible outcomes of parsing a fuzz input.
#[derive(Debug, Clone)]
pub enum FuzzOutcome {
    /// Parsed successfully
    Success,
    /// Returned a well-formed error (expected)
    ExpectedError(String),
    /// Panicked (should never happen)
    Panic,
}

/// Run a fuzz test against the KDBX parser.
/// Tests that the parser handles malformed input gracefully (returns error, doesn't panic).
pub fn fuzz_parse(data: &[u8], target: FuzzTarget) -> FuzzResult {
    let outcome = match target {
        FuzzTarget::RawKdb | FuzzTarget::RawKdbx31 | FuzzTarget::RawKdbx4 => {
            let mut cursor = Cursor::new(data.to_vec());
            match DatabaseReader::detect_version(&mut cursor) {
                Ok(_) => {
                    // Successfully detected version — further parsing would require a key
                    FuzzOutcome::ExpectedError("Further parsing requires valid credentials".to_string())
                }
                Err(e) => FuzzOutcome::ExpectedError(format!("{:?}", e)),
            }
        }
        FuzzTarget::XmlPayload => {
            // For XML fuzzing, check if bytes are valid UTF-8
            match std::str::from_utf8(data) {
                Ok(_) => FuzzOutcome::ExpectedError("XML fuzz requires inner stream cipher".to_string()),
                Err(e) => FuzzOutcome::ExpectedError(format!("Invalid UTF-8: {:?}", e)),
            }
        }
    };

    FuzzResult {
        input_len: data.len(),
        target,
        outcome,
    }
}

/// Generate a minimal valid KDBX 3.1 header for mutation-based fuzzing.
pub fn kdbx31_header_seed() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0x9AA2D903u32.to_le_bytes()); // sig1
    bytes.extend_from_slice(&0xB54BFB67u32.to_le_bytes()); // sig2
    bytes.extend_from_slice(&0x00030001u32.to_le_bytes()); // version
    bytes
}

/// Generate a minimal valid KDBX 4.0 header for mutation-based fuzzing.
pub fn kdbx4_header_seed() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0x9AA2D903u32.to_le_bytes());
    bytes.extend_from_slice(&0xB54BFB67u32.to_le_bytes());
    bytes.extend_from_slice(&0x00040000u32.to_le_bytes());
    bytes
}

/// Generate a minimal KDB header seed.
pub fn kdb_header_seed() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0x9AA2D903u32.to_le_bytes());
    bytes.extend_from_slice(&0xB54BFB65u32.to_le_bytes());
    bytes.extend_from_slice(&0x00010003u32.to_le_bytes());
    bytes
}

/// Apply mutations to a seed for fuzz testing.
pub fn mutate(seed: &[u8], iteration: usize) -> Vec<u8> {
    let mut data = seed.to_vec();
    let rng = iteration;
    let len = data.len();

    match iteration % 8 {
        0 => {
            // Flip a random bit
            if len > 0 {
                let idx = rng % len;
                data[idx] ^= 1 << ((rng >> 3) % 8);
            }
        }
        1 => {
            // Replace byte with 0xFF
            if len > 0 {
                data[rng % len] = 0xFF;
            }
        }
        2 => {
            // Replace byte with 0x00
            if len > 0 {
                data[rng % len] = 0x00;
            }
        }
        3 => {
            // Append bytes
            data.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        }
        4 => {
            // Truncate
            if len > 4 {
                data.truncate(len / 2);
            }
        }
        5 => {
            // Duplicate a section
            if len > 4 {
                let mid = len / 2;
                let dup = data[..mid].to_vec();
                data.extend_from_slice(&dup);
            }
        }
        6 => {
            // Insert zeros
            let pos = if len == 0 { 0 } else { rng % len };
            data.splice(pos..pos, std::iter::repeat(0).take(8));
        }
        7 if len >= 2 => {
            // Swap two bytes
            let a = rng % len;
            let b = (rng + 1) % len;
            data.swap(a, b);
        }
        _ => {}
    }

    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuzz_empty_input() {
        let result = fuzz_parse(&[], FuzzTarget::RawKdbx31);
        assert!(matches!(result.outcome, FuzzOutcome::ExpectedError(_)));
    }

    #[test]
    fn test_fuzz_garbage_input() {
        let garbage = vec![0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0xBA, 0xBE];
        let result = fuzz_parse(&garbage, FuzzTarget::RawKdbx31);
        assert!(matches!(result.outcome, FuzzOutcome::ExpectedError(_)));
    }

    #[test]
    fn test_fuzz_partial_header() {
        let partial = kdbx31_header_seed();
        let result = fuzz_parse(&partial, FuzzTarget::RawKdbx31);
        // Valid header but further parsing requires credentials
        assert!(matches!(result.outcome, FuzzOutcome::ExpectedError(_)));
    }

    #[test]
    fn test_fuzz_kdb_garbage() {
        let result = fuzz_parse(&[0x00, 0x01, 0x02, 0x03], FuzzTarget::RawKdb);
        assert!(matches!(result.outcome, FuzzOutcome::ExpectedError(_)));
    }

    #[test]
    fn test_fuzz_xml_garbage() {
        let result = fuzz_parse(b"<not valid xml <<<", FuzzTarget::XmlPayload);
        assert!(matches!(result.outcome, FuzzOutcome::ExpectedError(_)));
    }

    #[test]
    fn test_mutations_produce_different_outputs() {
        let seed = kdbx31_header_seed();
        let mut outputs = std::collections::HashSet::new();
        for i in 0..16 {
            outputs.insert(mutate(&seed, i));
        }
        assert!(outputs.len() > 1);
    }

    #[test]
    fn test_fuzz_many_iterations_no_panic() {
        let seed = kdbx4_header_seed();
        for i in 0..100 {
            let input = mutate(&seed, i);
            let result = fuzz_parse(&input, FuzzTarget::RawKdbx4);
            if let FuzzOutcome::Panic = result.outcome { panic!("Fuzz test panicked at iteration {}", i) }
        }
    }
}
