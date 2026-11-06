use crate::error::{Issue, PhvError};
use crate::write::Phv;

impl Phv {
    pub async fn check(&self) -> Result<Vec<Issue>, PhvError> {
        Ok(Vec::new())
    }
}
