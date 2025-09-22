#![cfg(feature = "surreal")]

use crate::errors::StorageError;
use surrealdb::error::{Api as ApiError, Db as DbError};
use surrealdb::Error as SurrealError;

pub fn map_surreal_error(err: SurrealError) -> StorageError {
    match err {
        SurrealError::Db(db) => map_db_error(db),
        SurrealError::Api(api) => map_api_error(api),
    }
}

fn map_db_error(err: DbError) -> StorageError {
    use DbError::*;
    let err_text = err.to_string();
    match err {
        Ds(msg) | Tx(msg) => StorageError::provider_unavailable(msg),
        TxRetryable | TxTooLarge | TxFailure | TxFinished | TxReadonly | TxConditionNotMet
        | TxKeyAlreadyExists | TxKeyTooLarge | TxValueTooLarge => {
            StorageError::provider_unavailable(err_text)
        }
        InvalidQuery(rendered) => StorageError::schema(rendered.to_string()),
        InvalidContent { .. }
        | InvalidMerge { .. }
        | InvalidPatch { .. }
        | PatchTest { .. }
        | InvalidParam { .. }
        | NsEmpty
        | DbEmpty
        | QueryEmpty
        | QueryRemaining
        | Deprecated(_)
        | Thrown(_)
        | HttpDisabled => StorageError::schema(err_text),
        RetryWithId(_) => StorageError::provider_unavailable(err_text),
        _ => StorageError::schema(err_text),
    }
}

fn map_api_error(err: ApiError) -> StorageError {
    use ApiError::*;
    match err {
        Http(msg) | Ws(msg) => StorageError::provider_unavailable(msg),
        Query(msg)
        | Scheme(msg)
        | InvalidRequest(msg)
        | InvalidParams(msg)
        | ParseError(msg)
        | InvalidSemanticVersion(msg)
        | InvalidUrl(msg) => StorageError::schema(msg),
        InvalidBindings(value) => StorageError::schema(format!("invalid bindings: {value}")),
        DuplicateRequestId(id) => StorageError::schema(format!("duplicate request id: {id}")),
        RangeOnRecordId | RangeOnObject | RangeOnArray | RangeOnEdges | RangeOnRange
        | RangeOnUnspecified => StorageError::schema("range query not permitted on resource"),
        InternalError(msg) => StorageError::provider_unavailable(msg),
        VersionMismatch { .. }
        | ConnectionUninitialised
        | AlreadyConnected
        | BackupsNotSupported => StorageError::provider_unavailable(err.to_string()),
        _ => StorageError::unknown(err.to_string()),
    }
}
