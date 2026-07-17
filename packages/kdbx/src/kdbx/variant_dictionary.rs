//! VariantDictionary binary format
//!
//! Key-value dictionary used in KDBX 4.0 headers and KDF parameters.

use std::collections::HashMap;
use std::io::{Read, Write};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

use crate::model::exception::{DatabaseError, DatabaseResult};

/// VariantDictionary type tags
mod vd_type {
    pub const NONE: u8 = 0x00;
    pub const UINT32: u8 = 0x04;
    pub const UINT64: u8 = 0x05;
    pub const BOOL: u8 = 0x08;
    pub const INT32: u8 = 0x0C;
    pub const INT64: u8 = 0x0D;
    pub const STRING: u8 = 0x18;
    pub const BYTE_ARRAY: u8 = 0x42;
}

const VD_VERSION: u16 = 0x0100;
const VDM_CRITICAL: u16 = 0xFF00;

/// A typed value in a VariantDictionary
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VdValue {
    UInt32(u32),
    UInt64(u64),
    Bool(bool),
    Int32(i32),
    Int64(i64),
    String(String),
    ByteArray(Vec<u8>),
}

/// VariantDictionary - a typed key-value store used in KDBX 4.0
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VariantDictionary {
    dict: HashMap<String, VdValue>,
}

impl VariantDictionary {
    pub fn new() -> Self {
        Self::default()
    }

    // ---- Getters ----

    pub fn get_uint32(&self, name: &str) -> Option<u32> {
        match self.dict.get(name)? {
            VdValue::UInt32(v) => Some(*v),
            _ => None,
        }
    }

    pub fn get_uint64(&self, name: &str) -> Option<u64> {
        match self.dict.get(name)? {
            VdValue::UInt64(v) => Some(*v),
            _ => None,
        }
    }

    pub fn get_bool(&self, name: &str) -> Option<bool> {
        match self.dict.get(name)? {
            VdValue::Bool(v) => Some(*v),
            _ => None,
        }
    }

    pub fn get_int32(&self, name: &str) -> Option<i32> {
        match self.dict.get(name)? {
            VdValue::Int32(v) => Some(*v),
            _ => None,
        }
    }

    pub fn get_int64(&self, name: &str) -> Option<i64> {
        match self.dict.get(name)? {
            VdValue::Int64(v) => Some(*v),
            _ => None,
        }
    }

    pub fn get_string(&self, name: &str) -> Option<&str> {
        match self.dict.get(name)? {
            VdValue::String(v) => Some(v),
            _ => None,
        }
    }

    pub fn get_byte_array(&self, name: &str) -> Option<&[u8]> {
        match self.dict.get(name)? {
            VdValue::ByteArray(v) => Some(v),
            _ => None,
        }
    }

    // ---- Setters ----

    pub fn set_uint32(&mut self, name: &str, value: u32) {
        self.dict.insert(name.to_string(), VdValue::UInt32(value));
    }

    pub fn set_uint64(&mut self, name: &str, value: u64) {
        self.dict.insert(name.to_string(), VdValue::UInt64(value));
    }

    pub fn set_bool(&mut self, name: &str, value: bool) {
        self.dict.insert(name.to_string(), VdValue::Bool(value));
    }

    pub fn set_int32(&mut self, name: &str, value: i32) {
        self.dict.insert(name.to_string(), VdValue::Int32(value));
    }

    pub fn set_int64(&mut self, name: &str, value: i64) {
        self.dict.insert(name.to_string(), VdValue::Int64(value));
    }

    pub fn set_string(&mut self, name: &str, value: &str) {
        self.dict
            .insert(name.to_string(), VdValue::String(value.to_string()));
    }

    pub fn set_byte_array(&mut self, name: &str, value: &[u8]) {
        self.dict
            .insert(name.to_string(), VdValue::ByteArray(value.to_vec()));
    }

    pub fn len(&self) -> usize {
        self.dict.len()
    }

    pub fn is_empty(&self) -> bool {
        self.dict.is_empty()
    }

    /// Deserialize a VariantDictionary from bytes.
    pub fn deserialize(data: &[u8]) -> DatabaseResult<Self> {
        let mut cursor = std::io::Cursor::new(data);
        let result = Self::read_from(&mut cursor)?;
        if cursor.position() != data.len() as u64 {
            return Err(DatabaseError::InvalidFormat(
                "Trailing VariantDictionary data".into(),
            ));
        }
        Ok(result)
    }

    /// Read a VariantDictionary from a stream.
    pub fn read_from<R: Read>(reader: &mut R) -> DatabaseResult<Self> {
        let dictionary = VariantDictionary::new();
        let version = reader.read_u16::<LittleEndian>()?;

        if (version & VDM_CRITICAL) > (VD_VERSION & VDM_CRITICAL) {
            return Err(DatabaseError::InvalidFormat(format!(
                "Unsupported VariantDictionary version: {version:#06x}"
            )));
        }

        // Read dictionary entries
        let mut dict = dictionary.dict;

        loop {
            let type_byte = reader.read_u8()?;
            if type_byte == vd_type::NONE {
                break;
            }

            let name_len = reader.read_u32::<LittleEndian>()? as usize;
            if name_len == 0 || name_len > 1024 {
                return Err(DatabaseError::InvalidFormat(
                    "VariantDictionary name length is out of range".into(),
                ));
            }
            let mut name_buf = vec![0u8; name_len];
            reader.read_exact(&mut name_buf)?;
            let name = String::from_utf8(name_buf)
                .map_err(|e| DatabaseError::InvalidFormat(format!("Invalid VD name: {e}")))?;

            let value_len = reader.read_u32::<LittleEndian>()? as usize;
            if value_len > crate::kdbx::limits::MAX_OUTER_HEADER_FIELD_SIZE {
                return Err(DatabaseError::InvalidFormat(
                    "VariantDictionary value is too large".into(),
                ));
            }
            let mut value_buf = vec![0u8; value_len];
            reader.read_exact(&mut value_buf)?;

            if dict.contains_key(&name) {
                return Err(DatabaseError::InvalidFormat(format!(
                    "Duplicate VariantDictionary key: {name}"
                )));
            }
            match type_byte {
                vd_type::UINT32 if value_len == 4 => {
                    let val = u32::from_le_bytes(value_buf.try_into().map_err(|_| {
                        DatabaseError::InvalidFormat("Invalid UINT32 length".into())
                    })?);
                    dict.insert(name, VdValue::UInt32(val));
                }
                vd_type::UINT64 if value_len == 8 => {
                    let val = u64::from_le_bytes(value_buf.try_into().map_err(|_| {
                        DatabaseError::InvalidFormat("Invalid UINT64 length".into())
                    })?);
                    dict.insert(name, VdValue::UInt64(val));
                }
                vd_type::BOOL if value_len == 1 => {
                    dict.insert(name, VdValue::Bool(value_buf[0] != 0));
                }
                vd_type::INT32 if value_len == 4 => {
                    let val = i32::from_le_bytes(value_buf.try_into().map_err(|_| {
                        DatabaseError::InvalidFormat("Invalid INT32 length".into())
                    })?);
                    dict.insert(name, VdValue::Int32(val));
                }
                vd_type::INT64 if value_len == 8 => {
                    let val = i64::from_le_bytes(value_buf.try_into().map_err(|_| {
                        DatabaseError::InvalidFormat("Invalid INT64 length".into())
                    })?);
                    dict.insert(name, VdValue::Int64(val));
                }
                vd_type::STRING => {
                    let val = String::from_utf8(value_buf)
                        .map_err(|e| DatabaseError::InvalidFormat(format!("Invalid UTF-8: {e}")))?;
                    dict.insert(name, VdValue::String(val));
                }
                vd_type::BYTE_ARRAY => {
                    dict.insert(name, VdValue::ByteArray(value_buf));
                }
                _ => {
                    return Err(DatabaseError::InvalidFormat(format!(
                        "Unknown or malformed VariantDictionary type {type_byte:#04x}"
                    )))
                }
            }
        }

        Ok(VariantDictionary { dict })
    }

    /// Serialize the VariantDictionary to bytes.
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        self.write_to(&mut buf)
            .expect("write to Vec should not fail");
        buf
    }

    /// Write the VariantDictionary to a stream.
    pub fn write_to<W: Write>(&self, writer: &mut W) -> DatabaseResult<()> {
        writer.write_u16::<LittleEndian>(VD_VERSION)?;

        for (name, vd) in &self.dict {
            let name_bytes = name.as_bytes();
            writer.write_u8(match vd {
                VdValue::UInt32(_) => vd_type::UINT32,
                VdValue::UInt64(_) => vd_type::UINT64,
                VdValue::Bool(_) => vd_type::BOOL,
                VdValue::Int32(_) => vd_type::INT32,
                VdValue::Int64(_) => vd_type::INT64,
                VdValue::String(_) => vd_type::STRING,
                VdValue::ByteArray(_) => vd_type::BYTE_ARRAY,
            })?;

            writer.write_u32::<LittleEndian>(name_bytes.len() as u32)?;
            writer.write_all(name_bytes)?;

            match vd {
                VdValue::UInt32(v) => {
                    writer.write_u32::<LittleEndian>(4)?;
                    writer.write_u32::<LittleEndian>(*v)?;
                }
                VdValue::UInt64(v) => {
                    writer.write_u32::<LittleEndian>(8)?;
                    writer.write_u64::<LittleEndian>(*v)?;
                }
                VdValue::Bool(v) => {
                    writer.write_u32::<LittleEndian>(1)?;
                    writer.write_u8(if *v { 1 } else { 0 })?;
                }
                VdValue::Int32(v) => {
                    writer.write_u32::<LittleEndian>(4)?;
                    writer.write_i32::<LittleEndian>(*v)?;
                }
                VdValue::Int64(v) => {
                    writer.write_u32::<LittleEndian>(8)?;
                    writer.write_i64::<LittleEndian>(*v)?;
                }
                VdValue::String(v) => {
                    let bytes = v.as_bytes();
                    writer.write_u32::<LittleEndian>(bytes.len() as u32)?;
                    writer.write_all(bytes)?;
                }
                VdValue::ByteArray(v) => {
                    writer.write_u32::<LittleEndian>(v.len() as u32)?;
                    writer.write_all(v)?;
                }
            }
        }

        writer.write_u8(vd_type::NONE)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_variant_dictionary_roundtrip() {
        let mut vd = VariantDictionary::new();
        vd.set_uint32("R", 500000);
        vd.set_uint64("M", 16777216);
        vd.set_bool("B", true);
        vd.set_string("S", "test");
        vd.set_byte_array("DATA", &[0x01, 0x02, 0x03]);

        let serialized = vd.serialize();
        let deserialized = VariantDictionary::deserialize(&serialized).unwrap();

        assert_eq!(deserialized.get_uint32("R"), Some(500000));
        assert_eq!(deserialized.get_uint64("M"), Some(16777216));
        assert_eq!(deserialized.get_bool("B"), Some(true));
        assert_eq!(deserialized.get_string("S"), Some("test"));
        assert_eq!(
            deserialized.get_byte_array("DATA"),
            Some(&[0x01, 0x02, 0x03][..])
        );
    }

    #[test]
    fn test_variant_dictionary_empty() {
        let vd = VariantDictionary::new();
        let serialized = vd.serialize();
        let deserialized = VariantDictionary::deserialize(&serialized).unwrap();
        assert!(deserialized.is_empty());
    }
}
