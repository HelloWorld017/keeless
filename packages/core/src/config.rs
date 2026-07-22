use serde::{Deserialize, Serialize};

use crate::{CoreError, Result};

pub const CONFIG_VERSION: u8 = 1;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PersistedConfig {
    pub version: u8,
    #[serde(default)]
    pub settings: keeless_schema::KeelessConfig,
}

impl PersistedConfig {
    pub fn validate(&self) -> Result<()> {
        if self.version != CONFIG_VERSION {
            return Err(CoreError::InvalidConfig("unsupported version".into()));
        }
        Ok(())
    }
}
