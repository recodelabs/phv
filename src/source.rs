use crate::error::PhvError;
use crate::store::Vault;
use serde_json::Value;

pub(crate) fn build_provenance(
    _vault: &Vault,
    _sha256: &str,
    _targets: &[String],
    _agent: &str,
    _recorded: &str,
) -> Result<Value, PhvError> {
    Err(PhvError::Usage("sources are not implemented yet".into()))
}
