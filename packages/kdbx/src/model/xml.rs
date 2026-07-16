//! Opaque XML extensions retained for lossless KDBX round-trips.

use std::collections::HashMap;

use uuid::Uuid;

use crate::model::core::node::NodeId;

/// An XML element not understood by this crate.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreservedXmlElement {
    pub(crate) start: String,
    pub(crate) name_len: usize,
    pub(crate) content: Vec<PreservedXmlContent>,
    pub(crate) empty: bool,
}

/// Content of an opaque XML element.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreservedXmlContent {
    Element(PreservedXmlElement),
    Text(String),
    CData(String),
    Comment(String),
    ProcessingInstruction(String),
    Protected(Vec<u8>),
}

/// Database-level XML extension points.
#[doc(hidden)]
#[derive(Debug, Default)]
pub struct DatabaseXmlExtensions {
    pub(crate) keepass_file: Vec<PreservedXmlElement>,
    pub(crate) meta: Vec<PreservedXmlElement>,
    pub(crate) memory_protection: Vec<PreservedXmlElement>,
    pub(crate) custom_icons: Vec<PreservedXmlElement>,
    pub(crate) custom_icon: HashMap<Uuid, Vec<PreservedXmlElement>>,
    pub(crate) root: Vec<PreservedXmlElement>,
    pub(crate) deleted_objects: Vec<PreservedXmlElement>,
    pub(crate) deleted_object: HashMap<NodeId, Vec<PreservedXmlElement>>,
}

/// Group-level XML extension points.
#[doc(hidden)]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupXmlExtensions {
    pub(crate) children: Vec<PreservedXmlElement>,
    pub(crate) times: Vec<PreservedXmlElement>,
}

/// Entry-level XML extension points.
#[doc(hidden)]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntryXmlExtensions {
    pub(crate) children: Vec<PreservedXmlElement>,
    pub(crate) times: Vec<PreservedXmlElement>,
    pub(crate) history: Vec<PreservedXmlElement>,
    pub(crate) auto_type: Vec<PreservedXmlElement>,
    pub(crate) associations: Vec<Vec<PreservedXmlElement>>,
    pub(crate) strings: HashMap<String, Vec<PreservedXmlElement>>,
    pub(crate) binaries: HashMap<String, Vec<PreservedXmlElement>>,
}

/// Custom-data XML extension points.
#[doc(hidden)]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CustomDataXmlExtensions {
    pub(crate) children: Vec<PreservedXmlElement>,
    pub(crate) items: HashMap<String, Vec<PreservedXmlElement>>,
}
