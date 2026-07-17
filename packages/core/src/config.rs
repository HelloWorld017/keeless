use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::{CoreError, Result, protocol::parse_public_key_bundle};

pub const CONFIG_VERSION: u8 = 1;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PersistedConfig {
    pub version: u8,
    #[serde(default)]
    pub settings: keeless_schema::KeelessConfig,
    pub identity: PersistedIdentity,
    #[serde(default)]
    pub approved_client_bundles: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PersistedIdentity {
    pub ed25519_signing_seed: String,
    pub x25519_static_secret: String,
}

impl Drop for PersistedIdentity {
    fn drop(&mut self) {
        self.ed25519_signing_seed.zeroize();
        self.x25519_static_secret.zeroize();
    }
}

impl PersistedConfig {
    pub fn validate(&self) -> Result<()> {
        if self.version != CONFIG_VERSION {
            return Err(CoreError::InvalidConfig("unsupported version".into()));
        }
        decode_32(&self.identity.ed25519_signing_seed)?;
        decode_32(&self.identity.x25519_static_secret)?;
        for bundle in &self.approved_client_bundles {
            parse_public_key_bundle(bundle)
                .ok_or_else(|| CoreError::InvalidConfig("invalid approved client bundle".into()))?;
        }
        Ok(())
    }
}

pub(crate) fn decode_32(value: &str) -> Result<Zeroizing<[u8; 32]>> {
    let bytes = Zeroizing::new(
        URL_SAFE_NO_PAD
            .decode(value)
            .map_err(|_| CoreError::InvalidConfig("invalid base64url secret".into()))?,
    );
    let value = bytes
        .as_slice()
        .try_into()
        .map_err(|_| CoreError::InvalidConfig("invalid secret length".into()))?;
    Ok(Zeroizing::new(value))
}

pub(crate) fn encode(value: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(value)
}
