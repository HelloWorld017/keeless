use super::*;

#[test]
fn credential_vault_wraps_and_debug_redacts() {
    let key = keeless_kdbx::CompositeKey::from_derived_key(
        keeless_kdbx::SecureArray::from_slice(&[42; 32]).unwrap(),
        [1; 32],
    );
    let vault = CredentialVault::wrap(&key).unwrap();
    assert!(vault.key_matches(&[42; 32]).unwrap());
    assert_eq!(
        format!("{vault:?}"),
        "CredentialVault { credential: \"[REDACTED]\" }"
    );
}

#[test]
fn prepared_next_preserves_kdf_cost_and_derives_a_distinct_reusable_key() {
    let credentials = CompositeCredentials::new()
        .with_password(b"correct")
        .unwrap();
    let mut database = Database::new(DatabaseVersion::KDBX31);
    let current = keeless_kdbx::initialize_database_key(&mut database, &credentials).unwrap();
    let parameters = database.kdf_parameters.as_ref().unwrap();
    let vault = CredentialVault::prepare(&current, &credentials, parameters).unwrap();
    let (next_parameters, next) = vault.restore_next().unwrap().unwrap();
    assert_ne!(next.kdf_fingerprint(), current.kdf_fingerprint());
    let mut expected_parameters = parameters.clone();
    expected_parameters.set_byte_array("S", next_parameters.get_byte_array("S").unwrap());
    assert_eq!(next_parameters, expected_parameters);
    let expected = credentials.derive_key(&next_parameters).unwrap();
    assert!(
        expected
            .with_key(|expected| next.with_key(|next| expected == next))
            .unwrap()
            .unwrap()
    );
    assert_eq!(
        vault.restore_key().unwrap().kdf_fingerprint(),
        current.kdf_fingerprint()
    );
    assert_eq!(
        vault.restore_next().unwrap().unwrap().1.kdf_fingerprint(),
        next.kdf_fingerprint()
    );
    assert!(
        CredentialVault::wrap(&next)
            .unwrap()
            .restore_next()
            .unwrap()
            .is_none()
    );
}
