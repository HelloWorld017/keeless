//! KDB/KDBX file I/O (reading and writing).
//!

// ─── Shared ─────────────────────────────────────────────────────────
pub mod header;

// ─── Readers ────────────────────────────────────────────────────────
pub mod kdb_reader;
pub mod kdbx31_reader;
pub mod kdbx4_reader;
pub mod reader;

// ─── Writers ────────────────────────────────────────────────────────
pub mod kdb_writer;
pub mod kdbx31_writer;
pub mod kdbx4_writer;
pub mod writer;

// ─── Re-exports ─────────────────────────────────────────────────────
pub use header::*;
pub use reader::DatabaseReader;
pub use writer::DatabaseWriter;
pub use kdb_reader::read_kdb;
pub use kdbx31_reader::read_kdbx31;
pub use kdbx4_reader::read_kdbx4;
pub use kdb_writer::write_kdb;
pub use kdbx31_writer::write_kdbx31;
pub use kdbx4_writer::write_kdbx4;
