//! KDF engine trait
//!

use uuid::Uuid;

use super::kdf_parameters::KdfParameters;
use crate::model::exception::DatabaseResult;

/// Key derivation function engine trait.
pub trait KdfEngine: Send + Sync {
    /// UUID identifying this KDF
    fn uuid(&self) -> Uuid;

    /// Derive a key from the master key using the given parameters.
    fn transform(&self, master_key: &[u8], params: &KdfParameters) -> DatabaseResult<Vec<u8>>;

    /// Randomize the salt in the parameters.
    fn randomize(&self, params: &mut KdfParameters);

    /// Get default KDF parameters
    fn default_parameters(&self) -> KdfParameters;

    /// Get the number of key transformation rounds.
    fn get_key_rounds(&self, params: &KdfParameters) -> u64;

    /// Set the number of key transformation rounds.
    fn set_key_rounds(&self, params: &mut KdfParameters, rounds: u64);

    /// Default key rounds
    fn default_key_rounds(&self) -> u64;

    /// Minimum key rounds
    fn min_key_rounds(&self) -> u64 {
        1
    }

    /// Maximum key rounds
    fn max_key_rounds(&self) -> u64 {
        u32::MAX as u64
    }

    /// Human-readable name
    fn name(&self) -> &str;
}
