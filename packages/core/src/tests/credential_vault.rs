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
