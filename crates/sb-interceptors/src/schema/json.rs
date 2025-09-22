use jsonschema::{JSONSchema, ValidationError};
use std::collections::HashMap;

#[derive(Default)]
pub struct JsonSchemaRegistry {
    schemas: HashMap<String, serde_json::Value>,
}

impl JsonSchemaRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        name: impl Into<String>,
        schema: serde_json::Value,
    ) -> Result<(), ValidationError> {
        self.schemas.insert(name.into(), schema);
        Ok(())
    }

    pub fn validate(&self, name: &str, payload: &serde_json::Value) -> Result<(), Vec<String>> {
        let Some(schema) = self.schemas.get(name) else {
            return Ok(());
        };
        let compiled = JSONSchema::compile(schema).map_err(|err| vec![err.to_string()])?;
        if let Err(errors) = compiled.validate(payload) {
            let messages = errors.map(|err| err.to_string()).collect::<Vec<_>>();
            return Err(messages);
        }
        Ok(())
    }
}
