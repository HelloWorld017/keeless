//! Sort order for database nodes
//!

/// Sort order for entries and groups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortNodeEnum {
    #[default]
    None,
    Name,
    NameDesc,
    CreationTime,
    CreationTimeDesc,
    LastModificationTime,
    LastModificationTimeDesc,
    LastAccessTime,
    LastAccessTimeDesc,
}
