use super::*;

#[test]
fn credential_vault_wraps_and_debug_redacts() {
    let raw = SecureArray::from_slice(&[42; 32]).unwrap();
    let vault = CredentialVault::wrap(&raw).unwrap();
    assert!(vault.raw_key_matches(&[42; 32]).unwrap());
    assert_eq!(
        format!("{vault:?}"),
        "CredentialVault { credential: \"[REDACTED]\" }"
    );
}
