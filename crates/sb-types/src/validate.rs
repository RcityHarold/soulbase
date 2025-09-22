use crate::envelope::Envelope;
use crate::scope::{Consent, Scope};
use crate::subject::Subject;
use crate::tenant::TenantId;

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ValidateError {
    #[error("empty_field:{0}")]
    EmptyField(&'static str),
    #[error("invalid_semver:{0}")]
    InvalidSemVer(String),
    #[error("tenant_mismatch")]
    TenantMismatch,
}

pub trait Validate {
    fn validate(&self) -> Result<(), ValidateError>;
}

impl Validate for Subject {
    fn validate(&self) -> Result<(), ValidateError> {
        if self.subject_id.0.is_empty() {
            return Err(ValidateError::EmptyField("subject_id"));
        }
        if self.tenant.0.is_empty() {
            return Err(ValidateError::EmptyField("tenant"));
        }
        Ok(())
    }
}

impl Validate for Scope {
    fn validate(&self) -> Result<(), ValidateError> {
        if self.resource.trim().is_empty() {
            return Err(ValidateError::EmptyField("scope.resource"));
        }
        if self.action.trim().is_empty() {
            return Err(ValidateError::EmptyField("scope.action"));
        }
        Ok(())
    }
}

impl Validate for Consent {
    fn validate(&self) -> Result<(), ValidateError> {
        for scope in &self.scopes {
            scope.validate()?;
        }
        Ok(())
    }
}

impl<T> Validate for Envelope<T> {
    fn validate(&self) -> Result<(), ValidateError> {
        if self.envelope_id.0.is_empty() {
            return Err(ValidateError::EmptyField("envelope_id"));
        }
        if self.partition_key.trim().is_empty() {
            return Err(ValidateError::EmptyField("partition_key"));
        }
        if let Err(err) = semver::Version::parse(&self.schema_ver) {
            return Err(ValidateError::InvalidSemVer(err.to_string()));
        }
        self.actor.validate()?;
        if let Some(consent) = &self.consent {
            consent.validate()?;
        }
        ensure_tenant_partition(&self.actor.tenant, &self.partition_key)
    }
}

fn ensure_tenant_partition(tenant: &TenantId, partition_key: &str) -> Result<(), ValidateError> {
    if tenant.0.is_empty() {
        return Err(ValidateError::EmptyField("tenant"));
    }
    if !partition_key.starts_with(tenant.as_ref()) {
        return Err(ValidateError::TenantMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::Id;
    use crate::subject::{Subject, SubjectKind};
    use crate::time::Timestamp;

    #[test]
    fn tenant_partition_mismatch_fails() {
        let actor = Subject {
            kind: SubjectKind::User,
            subject_id: Id("user".into()),
            tenant: TenantId("tenant_a".into()),
            claims: Default::default(),
        };
        let env = Envelope::new(
            Id("env".into()),
            Timestamp(1),
            "tenant_b:xyz".into(),
            actor,
            "1.0.0",
            (),
        );
        assert_eq!(env.validate(), Err(ValidateError::TenantMismatch));
    }
}
