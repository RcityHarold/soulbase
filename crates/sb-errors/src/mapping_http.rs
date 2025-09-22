use http::StatusCode;

use crate::model::ErrorObj;

pub fn to_http_status(err: &ErrorObj) -> StatusCode {
    StatusCode::from_u16(err.http_status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

pub fn into_http_response_body(err: &ErrorObj) -> (StatusCode, crate::render::PublicErrorView) {
    let status = to_http_status(err);
    (status, err.to_public())
}
