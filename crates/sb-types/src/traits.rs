use crate::envelope::Envelope;
use crate::id::{CausationId, CorrelationId};
use crate::subject::Subject;
use crate::time::Timestamp;

pub trait Versioned {
    fn schema_version(&self) -> &str;
}

pub trait Partitioned {
    fn partition_key(&self) -> &str;
}

pub trait Auditable {
    fn actor(&self) -> &Subject;
    fn produced_at(&self) -> Timestamp;
}

pub trait Causal {
    fn causation_id(&self) -> Option<&CausationId>;
    fn correlation_id(&self) -> Option<&CorrelationId>;
}

impl<T> Versioned for Envelope<T> {
    #[inline]
    fn schema_version(&self) -> &str {
        &self.schema_ver
    }
}

impl<T> Partitioned for Envelope<T> {
    #[inline]
    fn partition_key(&self) -> &str {
        &self.partition_key
    }
}

impl<T> Auditable for Envelope<T> {
    #[inline]
    fn actor(&self) -> &Subject {
        &self.actor
    }

    #[inline]
    fn produced_at(&self) -> Timestamp {
        self.produced_at
    }
}

impl<T> Causal for Envelope<T> {
    #[inline]
    fn causation_id(&self) -> Option<&CausationId> {
        self.causation_id.as_ref()
    }

    #[inline]
    fn correlation_id(&self) -> Option<&CorrelationId> {
        self.correlation_id.as_ref()
    }
}
