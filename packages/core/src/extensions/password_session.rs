use std::any::Any;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce, aead::Aead};
use keeless_kdbx::{CompositeKey, Database, SecureArray};
use zeroize::Zeroizing;

use super::CoreExtension;
use crate::{CoreError, Result, random_array};

const SESSION_VERSION: u8 = 1;
const SESSION_NONCE_LENGTH: usize = 24;
const SESSION_KEY_LENGTH: usize = 32;
const SESSION_TTL_MILLIS: u64 = 60_000;
const SESSION_AAD: &[u8] = b"keeless-password-session-v1";

pub(crate) struct PasswordSessionExtension {
    key: Option<SecureArray<SESSION_KEY_LENGTH>>,
    expires_at_millis: Option<u64>,
}

impl PasswordSessionExtension {
    pub(crate) fn new() -> Self {
        Self {
            key: None,
            expires_at_millis: None,
        }
    }

    pub(crate) fn create(&mut self, password: &[u8], now_millis: u64) -> Result<String> {
        self.revoke();

        let mut raw_key = random_array::<SESSION_KEY_LENGTH>()?;
        let key = SecureArray::from_array_mut(&mut raw_key)?;
        let nonce = random_array::<SESSION_NONCE_LENGTH>()?;
        let ciphertext = key
            .unlock(|key| {
                XChaCha20Poly1305::new(key.into()).encrypt(
                    XNonce::from_slice(&nonce),
                    chacha20poly1305::aead::Payload {
                        msg: password,
                        aad: SESSION_AAD,
                    },
                )
            })?
            .map_err(|_| CoreError::Crypto)?;
        let ciphertext = Zeroizing::new(ciphertext);

        let mut token = Zeroizing::new(Vec::with_capacity(1 + nonce.len() + ciphertext.len()));
        token.push(SESSION_VERSION);
        token.extend_from_slice(&nonce);
        token.extend_from_slice(&ciphertext);

        self.key = Some(key);
        self.expires_at_millis = Some(now_millis.saturating_add(SESSION_TTL_MILLIS));
        Ok(URL_SAFE_NO_PAD.encode(&*token))
    }

    pub(crate) fn resolve_argument(
        &mut self,
        password: Option<String>,
        password_session: Option<String>,
        now_millis: u64,
    ) -> Result<Option<Zeroizing<Vec<u8>>>> {
        self.expire(now_millis);
        match (password, password_session) {
            (Some(_), Some(_)) => Err(CoreError::PasswordAndSession),
            (Some(password), None) => Ok(Some(Zeroizing::new(password.into_bytes()))),
            (None, Some(password_session)) => self.decrypt(&password_session, now_millis).map(Some),
            (None, None) => Ok(None),
        }
    }

    fn decrypt(&mut self, token: &str, now_millis: u64) -> Result<Zeroizing<Vec<u8>>> {
        self.expire(now_millis);
        let key = self.key.as_ref().ok_or(CoreError::InvalidPasswordSession)?;
        let token = Zeroizing::new(
            URL_SAFE_NO_PAD
                .decode(token)
                .map_err(|_| CoreError::InvalidPasswordSession)?,
        );
        if token.len() < 1 + SESSION_NONCE_LENGTH + 16 || token[0] != SESSION_VERSION {
            return Err(CoreError::InvalidPasswordSession);
        }

        let nonce = XNonce::from_slice(&token[1..1 + SESSION_NONCE_LENGTH]);
        let ciphertext = &token[1 + SESSION_NONCE_LENGTH..];
        let plaintext = key
            .unlock(|key| {
                XChaCha20Poly1305::new(key.into()).decrypt(
                    nonce,
                    chacha20poly1305::aead::Payload {
                        msg: ciphertext,
                        aad: SESSION_AAD,
                    },
                )
            })?
            .map_err(|_| CoreError::InvalidPasswordSession)?;
        Ok(Zeroizing::new(plaintext))
    }

    pub(crate) fn revoke(&mut self) {
        self.key = None;
        self.expires_at_millis = None;
    }

    fn expire(&mut self, now_millis: u64) {
        if self
            .expires_at_millis
            .is_some_and(|expires_at| now_millis >= expires_at)
        {
            self.revoke();
        }
    }
}

impl CoreExtension for PasswordSessionExtension {
    fn unlock(&mut self, _: &Database, _: &CompositeKey) -> Result<()> {
        Ok(())
    }

    fn lock(&mut self) {
        self.revoke();
    }

    fn tick(&mut self, monotonic_millis: u64) {
        self.expire(monotonic_millis);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_encrypt_passwords_and_replace_previous_keys() {
        let mut extension = PasswordSessionExtension::new();
        let first = extension.create(b"first", 100).unwrap();
        assert_eq!(extension.decrypt(&first, 100).unwrap().as_slice(), b"first");

        let second = extension.create(b"second", 100).unwrap();
        assert!(matches!(
            extension.decrypt(&first, 100),
            Err(CoreError::InvalidPasswordSession)
        ));
        assert_eq!(
            extension.decrypt(&second, 100).unwrap().as_slice(),
            b"second"
        );
    }

    #[test]
    fn sessions_expire_at_the_monotonic_deadline_and_can_be_revoked() {
        let mut extension = PasswordSessionExtension::new();
        let token = extension.create(b"secret", 100).unwrap();

        extension.tick(60_099);
        assert!(extension.decrypt(&token, 60_099).is_ok());
        extension.tick(60_100);
        assert!(matches!(
            extension.decrypt(&token, 60_100),
            Err(CoreError::InvalidPasswordSession)
        ));

        let token = extension.create(b"secret", 100).unwrap();
        extension.revoke();
        assert!(matches!(
            extension.decrypt(&token, 100),
            Err(CoreError::InvalidPasswordSession)
        ));
    }

    #[test]
    fn password_arguments_reject_ambiguous_credentials() {
        let mut extension = PasswordSessionExtension::new();
        assert!(matches!(
            extension.resolve_argument(Some("password".into()), Some("session".into()), 100,),
            Err(CoreError::PasswordAndSession)
        ));
    }
}
