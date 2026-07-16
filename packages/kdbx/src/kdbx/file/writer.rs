//! Database file writer
//!

use std::io::Write;

use byteorder::{LittleEndian, WriteBytesExt};

use crate::model::db::database::DatabaseVersion;
use crate::model::exception::DatabaseResult;

/// Database file writer.
pub struct DatabaseWriter;

impl DatabaseWriter {
    /// Write the KDBX file signature and version header.
    pub fn write_signature(
        writer: &mut impl Write,
        version: DatabaseVersion,
    ) -> DatabaseResult<()> {
        writer.write_u32::<LittleEndian>(super::header::KDBX_SIGNATURE_1)?;

        let sig2 = match version {
            DatabaseVersion::KDB => super::header::KDB_SIGNATURE_2,
            _ => super::header::KDBX_SIGNATURE_2,
        };
        writer.write_u32::<LittleEndian>(sig2)?;

        let ver = match version {
            DatabaseVersion::KDB => 0x00010003,
            DatabaseVersion::KDBX31 => 0x00030001,
            DatabaseVersion::KDBX4 => 0x00040001, // FILE_VERSION_4001
        };
        writer.write_u32::<LittleEndian>(ver)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    use crate::kdbx::file::reader::DatabaseReader;
    use std::io::Cursor;

    #[test]
    fn test_write_kdbx31_signature() {
        let mut buf = Vec::new();
        DatabaseWriter::write_signature(&mut buf, DatabaseVersion::KDBX31).unwrap();

        let mut cursor = Cursor::new(buf);
        let version = DatabaseReader::detect_version(&mut cursor).unwrap();
        assert_eq!(version, DatabaseVersion::KDBX31);
    }

    #[test]
    fn test_write_kdbx4_signature() {
        let mut buf = Vec::new();
        DatabaseWriter::write_signature(&mut buf, DatabaseVersion::KDBX4).unwrap();

        let mut cursor = Cursor::new(buf);
        let version = DatabaseReader::detect_version(&mut cursor).unwrap();
        assert_eq!(version, DatabaseVersion::KDBX4);
    }
}
