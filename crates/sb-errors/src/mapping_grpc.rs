use tonic::{Code, Status};

use crate::model::ErrorObj;

pub fn to_grpc_status(err: &ErrorObj) -> Status {
    let code = err
        .grpc_status
        .and_then(Code::from_i32)
        .unwrap_or(Code::Internal);
    Status::new(code, err.message_user.clone())
}
