//! KDBX-specific modules: file I/O, KDF engines, merge, repair, search, fuzz testing, stream processing, XML serialization, signatures, and variant dictionary.

pub mod file;
pub mod fuzz;
pub mod kdf;
pub(crate) mod limits;
pub mod merge;
pub mod repair;
pub mod search;
pub mod signature;
pub mod stream;
pub mod variant_dictionary;
pub mod xml;
