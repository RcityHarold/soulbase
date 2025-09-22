#[cfg(feature = "schema-json")]
pub mod json;

#[cfg(feature = "schema-json")]
pub use json::JsonSchemaRegistry;

#[cfg(not(feature = "schema-json"))]
#[derive(Clone, Default)]
pub struct JsonSchemaRegistry;

#[cfg(not(feature = "schema-json"))]
impl JsonSchemaRegistry {
    pub fn new() -> Self {
        Self
    }

    pub fn register(
        &mut self,
        _name: impl Into<String>,
        _schema: serde_json::Value,
    ) -> Result<(), ()> {
        Ok(())
    }

    pub fn validate(&self, _name: &str, _payload: &serde_json::Value) -> Result<(), Vec<String>> {
        Ok(())
    }
}
