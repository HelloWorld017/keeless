pub(crate) mod add_entry;
pub(crate) mod add_entry_from_template;
pub(crate) mod add_group;
mod cache;
pub(crate) mod delete_entry;
pub(crate) mod delete_group;
pub(crate) mod delete_tag;
mod journal;
pub(crate) mod move_entry;
pub(crate) mod move_group;
pub(crate) mod rename_group;
pub(crate) mod update_entry;
pub(crate) mod update_group;
pub(crate) mod update_tag_style;

use keeless_kdbx::{CompositeKey, Database};
use serde::{Deserialize, Serialize};

use crate::Result;

use journal::mutate;
pub(crate) use journal::{MutationCoordinator, replay_lines};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Mutation {
    AddEntry(add_entry::Mutation),
    AddEntryFromTemplate(add_entry_from_template::Mutation),
    AddGroup(add_group::Mutation),
    DeleteEntry(delete_entry::Mutation),
    DeleteGroup(delete_group::Mutation),
    MoveEntry(move_entry::Mutation),
    MoveGroup(move_group::Mutation),
    RenameGroup(rename_group::Mutation),
    UpdateGroup(update_group::Mutation),
    UpdateEntry(update_entry::Mutation),
    UpdateTagStyles(update_tag_style::Mutation),
}

fn apply(database: &mut Database, mutation: &Mutation, key: &CompositeKey) -> Result<()> {
    match mutation {
        Mutation::AddEntry(mutation) => add_entry::apply(database, mutation),
        Mutation::AddEntryFromTemplate(mutation) => {
            add_entry_from_template::apply(database, mutation, key)
        }
        Mutation::AddGroup(mutation) => add_group::apply(database, mutation),
        Mutation::DeleteEntry(mutation) => delete_entry::apply(database, mutation),
        Mutation::DeleteGroup(mutation) => delete_group::apply(database, mutation),
        Mutation::MoveEntry(mutation) => move_entry::apply(database, mutation),
        Mutation::MoveGroup(mutation) => move_group::apply(database, mutation),
        Mutation::RenameGroup(mutation) => rename_group::apply(database, mutation),
        Mutation::UpdateGroup(mutation) => update_group::apply(database, mutation),
        Mutation::UpdateEntry(mutation) => update_entry::apply(database, mutation, key),
        Mutation::UpdateTagStyles(mutation) => update_tag_style::apply(database, mutation),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keeless_kdbx::NodeId;
    use uuid::Uuid;

    #[test]
    fn mutation_serialization_keeps_flat_type_and_fields() {
        let mutation = Mutation::MoveGroup(move_group::Mutation {
            id: NodeId::from_uuid(Uuid::from_u128(1)),
            parent: NodeId::from_uuid(Uuid::from_u128(2)),
            index: 3,
            timestamp_ms: 4,
        });

        assert_eq!(
            serde_json::to_value(mutation).unwrap(),
            serde_json::json!({
                "type": "move_group",
                "id": "00000000-0000-0000-0000-000000000001",
                "parent": "00000000-0000-0000-0000-000000000002",
                "index": 3,
                "timestamp_ms": 4
            })
        );
    }
}
