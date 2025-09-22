use async_trait::async_trait;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use sb_types::prelude::{Id, TenantId};

use crate::errors::{TxError, TxResult};
use crate::model::{SagaDefinition, SagaInstance, SagaState, SagaStepState, StepState};
use crate::util::now_ms;

#[async_trait]
pub trait SagaStore: Send + Sync {
    async fn insert(&self, saga: SagaInstance) -> TxResult<()>;
    async fn load(&self, id: &Id) -> TxResult<Option<SagaInstance>>;
    async fn save(&self, saga: &SagaInstance) -> TxResult<()>;
}

#[async_trait]
pub trait SagaParticipant: Send + Sync {
    async fn execute(&self, uri: &str, saga: &SagaInstance) -> Result<bool, TxError>;
    async fn compensate(&self, uri: &str, saga: &SagaInstance) -> Result<bool, TxError>;
}

pub struct SagaOrchestrator<S, P>
where
    S: SagaStore,
    P: SagaParticipant,
{
    pub store: S,
    pub participant: P,
}

impl<S, P> SagaOrchestrator<S, P>
where
    S: SagaStore,
    P: SagaParticipant,
{
    pub async fn start(
        &self,
        tenant: &TenantId,
        def: &SagaDefinition,
        timeout_at: Option<i64>,
    ) -> TxResult<Id> {
        let id = new_saga_id();
        let steps = def
            .steps
            .clone()
            .into_iter()
            .map(SagaStepState::new)
            .collect::<Vec<_>>();
        let now = now_ms();
        let saga = SagaInstance {
            id: id.clone(),
            tenant: tenant.clone(),
            state: SagaState::Running,
            def_name: def.name.clone(),
            steps,
            cursor: 0,
            created_at: now,
            updated_at: now,
            timeout_at,
        };
        self.store.insert(saga).await?;
        Ok(id)
    }

    pub async fn tick(&self, id: &Id) -> TxResult<()> {
        let mut saga = match self.store.load(id).await? {
            Some(s) => s,
            None => return Ok(()),
        };

        match saga.state {
            SagaState::Running => self.tick_running(&mut saga).await?,
            SagaState::Compensating => self.tick_compensating(&mut saga).await?,
            SagaState::Paused | SagaState::Completed | SagaState::Failed | SagaState::Cancelled => {
            }
        }

        saga.updated_at = now_ms();
        self.store.save(&saga).await
    }

    async fn tick_running(&self, saga: &mut SagaInstance) -> TxResult<()> {
        if saga.cursor >= saga.steps.len() {
            saga.state = SagaState::Completed;
            return Ok(());
        }

        let idx = saga.cursor;
        let step_state = saga.steps.get(idx).cloned();
        if let Some(step_state) = step_state {
            if step_state.state == StepState::Succeeded {
                saga.cursor += 1;
                if saga.cursor >= saga.steps.len() {
                    saga.state = SagaState::Completed;
                }
                return Ok(());
            }
        }

        let action_uri = {
            let step = &mut saga.steps[idx];
            step.state = StepState::InFlight;
            step.started_at.get_or_insert(now_ms());
            step.def.action_uri.clone()
        };

        let result = self.participant.execute(&action_uri, saga).await;
        let step = &mut saga.steps[idx];

        match result {
            Ok(true) => {
                step.state = StepState::Succeeded;
                step.completed_at = Some(now_ms());
                saga.cursor += 1;
                if saga.cursor >= saga.steps.len() {
                    saga.state = SagaState::Completed;
                }
            }
            Ok(false) => {
                step.state = StepState::Failed;
                step.last_error = Some("step returned failure".to_string());
                saga.state = SagaState::Compensating;
                saga.cursor = idx + 1;
            }
            Err(err) => {
                step.state = StepState::Failed;
                step.last_error = Some(err.to_string());
                saga.state = SagaState::Compensating;
                saga.cursor = idx + 1;
            }
        }

        Ok(())
    }

    async fn tick_compensating(&self, saga: &mut SagaInstance) -> TxResult<()> {
        loop {
            if saga.cursor == 0 {
                saga.state = SagaState::Cancelled;
                return Ok(());
            }

            let idx = saga.cursor - 1;
            let step_state = saga.steps.get(idx).cloned();
            if matches!(
                step_state.as_ref().map(|s| s.state),
                Some(StepState::Compensated) | Some(StepState::Skipped)
            ) {
                saga.cursor = idx;
                continue;
            }

            let (uri, should_skip) = {
                let step = &mut saga.steps[idx];
                match step.def.compensate_uri.clone() {
                    Some(uri) => (uri, false),
                    None => {
                        step.state = StepState::Skipped;
                        (String::new(), true)
                    }
                }
            };

            if should_skip {
                saga.cursor = idx;
                if saga.cursor == 0 {
                    saga.state = SagaState::Failed;
                }
                continue;
            }

            let result = self.participant.compensate(&uri, saga).await;
            let step = &mut saga.steps[idx];

            match result {
                Ok(true) => {
                    step.state = StepState::Compensated;
                    step.completed_at = Some(now_ms());
                    saga.cursor = idx;
                    continue;
                }
                Ok(false) => {
                    step.last_error = Some("compensate returned failure".to_string());
                    saga.state = SagaState::Failed;
                    return Ok(());
                }
                Err(err) => {
                    step.last_error = Some(err.to_string());
                    saga.state = SagaState::Failed;
                    return Ok(());
                }
            }
        }
    }
}

fn new_saga_id() -> Id {
    let mut rng = thread_rng();
    let suffix: String = (0..16)
        .map(|_| char::from(rng.sample(Alphanumeric)))
        .collect();
    format!("sg-{suffix}").as_str().into()
}
