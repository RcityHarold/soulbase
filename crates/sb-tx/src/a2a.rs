use sb_types::prelude::{Id, TenantId};

use crate::errors::TxResult;
use crate::model::OutboxMessage;

/// Hooks for integrating with soulbase-a2a ledger / signature workflows.
pub trait A2AHooks: Send + Sync {
    fn on_outbox_dead_letter(
        &self,
        _tenant: &TenantId,
        _message: &OutboxMessage,
        _error_code: Option<&str>,
    ) -> TxResult<()> {
        Ok(())
    }

    fn on_outbox_replay(&self, _tenant: &TenantId, _message_id: &Id) -> TxResult<()> {
        Ok(())
    }
}

#[derive(Default)]
pub struct NoopA2AHooks;

impl A2AHooks for NoopA2AHooks {}
