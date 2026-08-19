//! Database file reader
//!

use std::io::Read;

use byteorder::{LittleEndian, ReadBytesExt};

use super::header::*;
use crate::crypto::compression::CompressionAlgorithm;
use crate::crypto::encryption_algorithm::EncryptionAlgorithm;
use crate::model::db::database::DatabaseVersion;
use crate::model::exception::{DatabaseError, DatabaseResult};

/// Read wrapper that copies all bytes consumed from the inner reader.
pub(crate) struct TeeReader<'a, R> {
    inner: &'a mut R,
    sink: &'a mut Vec<u8>,
}

impl<'a, R: Read> TeeReader<'a, R> {
    pub(crate) fn new(inner: &'a mut R, sink: &'a mut Vec<u8>) -> Self {
        Self { inner, sink }
    }
}

impl<R: Read> Read for TeeReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.sink.extend_from_slice(&buf[..n]);
        Ok(n)
    }
}

/// Database file reader.
pub struct DatabaseReader;

impl DatabaseReader {
    /// Detect the database format from file signature and version.
    pub fn detect_version(reader: &mut impl Read) -> DatabaseResult<DatabaseVersion> {
        let sig1 = reader.read_u32::<LittleEndian>()?;
        let sig2 = reader.read_u32::<LittleEndian>()?;
        let version = reader.read_u32::<LittleEndian>()?;

        if sig1 != KDBX_SIGNATURE_1 {
            return Err(DatabaseError::InvalidSignature(format!(
                "Expected {KDBX_SIGNATURE_1:#010x}, got {sig1:#010x}"
            )));
        }

        if sig2 == KDB_SIGNATURE_2 {
            Ok(DatabaseVersion::KDB)
        } else if sig2 == KDBX_SIGNATURE_2 {
            // FILE_VERSION format: 0x000XYYZZ
            // KDBX 3.1: 0x00030001 → major nibble in upper 16 bits
            // KDBX 4.0: 0x00040000/0x00040001
            let ver_major = (version >> 16) & 0xFFFF;
            match ver_major {
                3 => Ok(DatabaseVersion::KDBX31),
                4 => Ok(DatabaseVersion::KDBX4),
                _ => Err(DatabaseError::InvalidVersion(format!(
                    "Unsupported KDBX version: {version:#010x}"
                ))),
            }
        } else {
            Err(DatabaseError::InvalidSignature(format!(
                "Expected {KDBX_SIGNATURE_2:#010x} or {KDB_SIGNATURE_2:#010x}, got {sig2:#010x}"
            )))
        }
    }

    /// Read a KDBX 3.1 outer header from the stream.
    pub fn read_kdbx31_header(reader: &mut impl Read) -> DatabaseResult<KdbxHeader31> {
        let mut header = KdbxHeader31 {
            version: FILE_VERSION_31,
            encryption_algorithm: EncryptionAlgorithm::AesRijndael,
            compression: CompressionAlgorithm::Gzip,
            master_seed: Vec::new(),
            transform_seed: Vec::new(),
            transform_rounds: 500_000,
            encryption_iv: Vec::new(),
            inner_random_stream_key: Vec::new(),
            stream_start_bytes: Vec::new(),
            inner_random_stream: CrsAlgorithm::Salsa20,
        };

        loop {
            let field_id = reader.read_u8()?;
            let field_size = reader.read_u16::<LittleEndian>()? as usize;

            if field_id == header_field_31::END_OF_HEADER {
                // KeePass writes a four-byte CRLF marker in this field. Older
                // Keeless output left it empty, so consume either form.
                let mut marker = vec![0u8; field_size];
                reader.read_exact(&mut marker)?;
                break;
            }

            if field_size > 0 {
                let mut data = vec![0u8; field_size];
                reader.read_exact(&mut data)?;

                match field_id {
                    header_field_31::CIPHER_ID => {
                        let uuid = uuid::Uuid::from_slice(&data)
                            .map_err(|e| DatabaseError::InvalidFormat(e.to_string()))?;
                        header.encryption_algorithm = EncryptionAlgorithm::from_uuid(&uuid)
                            .unwrap_or(EncryptionAlgorithm::AesRijndael);
                    }
                    header_field_31::COMPRESSION_FLAGS => {
                        if data.len() >= 4 {
                            let flags = u32::from_le_bytes(data[..4].try_into().map_err(|_| {
                                DatabaseError::InvalidFormat("Invalid COMPRESSION_FLAGS".into())
                            })?);
                            header.compression = CompressionAlgorithm::from_id(flags)
                                .unwrap_or(CompressionAlgorithm::Gzip);
                        }
                    }
                    header_field_31::MASTER_SEED => header.master_seed = data,
                    header_field_31::TRANSFORM_SEED => header.transform_seed = data,
                    header_field_31::TRANSFORM_ROUNDS => {
                        if data.len() >= 8 {
                            header.transform_rounds =
                                u64::from_le_bytes(data[..8].try_into().map_err(|_| {
                                    DatabaseError::InvalidFormat("Invalid TRANSFORM_ROUNDS".into())
                                })?);
                        }
                    }
                    header_field_31::ENCRYPTION_IV => header.encryption_iv = data,
                    header_field_31::INNER_RANDOM_STREAM_KEY => {
                        header.inner_random_stream_key = data
                    }
                    header_field_31::STREAM_START_BYTES => header.stream_start_bytes = data,
                    header_field_31::INNER_RANDOM_STREAM_ID if data.len() >= 4 => {
                        let id = u32::from_le_bytes(data[..4].try_into().map_err(|_| {
                            DatabaseError::InvalidFormat("Invalid INNER_RANDOM_STREAM_ID".into())
                        })?);
                        header.inner_random_stream =
                            CrsAlgorithm::from_id(id).unwrap_or(CrsAlgorithm::Salsa20);
                    }
                    _ => {} // Skip unknown fields
                }
            }
        }

        Ok(header)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_detect_kdbx31_signature() {
        let mut data = Vec::new();
        data.extend_from_slice(&KDBX_SIGNATURE_1.to_le_bytes());
        data.extend_from_slice(&KDBX_SIGNATURE_2.to_le_bytes());
        // KDBX 3.1: version should have major=3 in high 16 bits
        // Actual FILE_VERSION_31 is 0x00030001, major = 0x0003
        data.extend_from_slice(&0x00030001u32.to_le_bytes());

        let mut cursor = Cursor::new(data);
        let version = DatabaseReader::detect_version(&mut cursor).unwrap();
        assert_eq!(version, DatabaseVersion::KDBX31);
    }

    #[test]
    fn test_detect_kdbx4_signature() {
        let mut data = Vec::new();
        data.extend_from_slice(&KDBX_SIGNATURE_1.to_le_bytes());
        data.extend_from_slice(&KDBX_SIGNATURE_2.to_le_bytes());
        data.extend_from_slice(&0x00040000u32.to_le_bytes()); // v4

        let mut cursor = Cursor::new(data);
        let version = DatabaseReader::detect_version(&mut cursor).unwrap();
        assert_eq!(version, DatabaseVersion::KDBX4);
    }

    #[test]
    fn test_detect_kdb_signature() {
        let mut data = Vec::new();
        data.extend_from_slice(&KDB_SIGNATURE_1.to_le_bytes());
        data.extend_from_slice(&KDB_SIGNATURE_2.to_le_bytes());
        data.extend_from_slice(&0x00010003u32.to_le_bytes());

        let mut cursor = Cursor::new(data);
        let version = DatabaseReader::detect_version(&mut cursor).unwrap();
        assert_eq!(version, DatabaseVersion::KDB);
    }

    #[test]
    fn test_invalid_signature() {
        let mut data = Vec::new();
        data.extend_from_slice(&0xDEADBEEFu32.to_le_bytes());
        data.extend_from_slice(&0xDEADBEEFu32.to_le_bytes());
        data.extend_from_slice(&0x00030001u32.to_le_bytes());

        let mut cursor = Cursor::new(data);
        assert!(DatabaseReader::detect_version(&mut cursor).is_err());
    }
}
