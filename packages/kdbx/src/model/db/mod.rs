//! Database core: database structure, composite key, and change tracking.

pub mod change_tracker;
pub mod composite_key;
pub mod database;

pub use database::{Database, DatabaseVersion};
pub use composite_key::{CompositeKey, MasterCredential};
pub use change_tracker::{ChangeTracker, ChangeType, ChangeRecord, DiffResult};
