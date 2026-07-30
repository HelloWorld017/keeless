use super::*;

#[tokio::test]
async fn permanently_removes_nested_entries_and_groups() {
    let (mut core, ids) = query_core().await;
    let root_entry = schema_id(ids.root_entry);
    let nested_entry = schema_id(ids.nested_entry);

    operations::mutations::delete_entry::run(
        &mut core,
        DeleteEntryArgs {
            entry_id: root_entry.clone(),
            permanent: false,
        },
    )
    .await
    .unwrap();
    operations::mutations::delete_group::run(
        &mut core,
        DeleteGroupArgs {
            group_id: schema_id(ids.child_group),
        },
    )
    .await
    .unwrap();

    operations::mutations::empty_recycle_bin::run(&mut core, EmptyRecycleBinArgs {})
        .await
        .unwrap();

    assert!(
        operations::get_trash_entries::run(&mut core)
            .unwrap()
            .entries
            .is_empty()
    );
    let database = core.handle.as_ref().unwrap().database();
    assert!(database.get_entry(&model_id(root_entry)).is_none());
    assert!(database.get_entry(&model_id(nested_entry)).is_none());
    assert!(
        database
            .get_group(&NodeId::from_uuid(ids.child_group))
            .is_none()
    );
}
