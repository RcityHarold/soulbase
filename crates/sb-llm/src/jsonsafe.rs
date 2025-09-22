use crate::chat::JsonSchema;
use crate::errors::LlmError;
use serde_json::Value;

#[derive(Clone, Debug)]
pub enum StructOutPolicy {
    Off,
    StrictReject,
    StrictRepair { max_attempts: u8 },
}

pub fn enforce_json(candidate: &str, policy: &StructOutPolicy) -> Result<Value, LlmError> {
    match policy {
        StructOutPolicy::Off => serde_json::from_str(candidate)
            .map_err(|err| LlmError::schema(&format!("json parse (off): {err}"))),
        StructOutPolicy::StrictReject => serde_json::from_str(candidate)
            .map_err(|err| LlmError::schema(&format!("json parse: {err}"))),
        StructOutPolicy::StrictRepair { max_attempts } => {
            let mut text = candidate.trim().to_string();
            let mut attempts = 0u8;
            loop {
                match serde_json::from_str::<Value>(&text) {
                    Ok(value) => return Ok(value),
                    Err(_err) if attempts < *max_attempts => {
                        attempts += 1;
                        text = text.trim().trim_matches('"').to_string();
                        if text.is_empty() {
                            return Err(LlmError::schema("json parse after repair: empty"));
                        }
                    }
                    Err(err) => {
                        return Err(LlmError::schema(&format!("json parse after repair: {err}")));
                    }
                }
            }
        }
    }
}

#[allow(unused_variables)]
pub fn validate_against_schema(value: &Value, schema: &Option<JsonSchema>) -> Result<(), LlmError> {
    #[cfg(feature = "schema-json")]
    {
        if let Some(schema) = schema {
            schemars::validate::Validate::validate(schema, value)
                .map_err(|err| LlmError::schema(&format!("schema validation failed: {err}")))?;
        }
    }
    Ok(())
}
