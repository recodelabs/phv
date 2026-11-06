use crate::error::PhvError;
use crate::write::Phv;

pub async fn serve(_phv: Phv, _port: u16) -> Result<(), PhvError> {
    Err(PhvError::Usage("the API is not implemented yet".into()))
}
