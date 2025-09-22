use crate::errors::SandboxError;
use crate::model::Budget;
use async_trait::async_trait;

#[async_trait]
pub trait BudgetMeter: Send + Sync {
    async fn reserve(&self, request: &Budget) -> Result<(), SandboxError>;
    async fn commit(&self, used: &Budget);
    async fn rollback(&self, used: &Budget);
}

#[derive(Default)]
pub struct NoopBudgetMeter;

#[async_trait]
impl BudgetMeter for NoopBudgetMeter {
    async fn reserve(&self, _request: &Budget) -> Result<(), SandboxError> {
        Ok(())
    }

    async fn commit(&self, _used: &Budget) {}

    async fn rollback(&self, _used: &Budget) {}
}
