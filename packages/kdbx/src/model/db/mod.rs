//! Database core: database structure, composite key, and change tracking.

pub mod change_tracker;
pub mod composite_key;
pub mod database;

pub use change_tracker::{ChangeRecord, ChangeTracker, ChangeType, DiffResult};
pub use composite_key::{CompositeKey, MasterCredential};
pub use database::{Database, DatabaseVersion, EntryFieldSelector, EntryFieldUpdate};
