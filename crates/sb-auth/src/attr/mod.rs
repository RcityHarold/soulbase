use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

use crate::model::{AttributeMap, ResourceUrn};
use sb_types::prelude::Subject;

#[async_trait]
pub trait AttributeProvider: Send + Sync {
    async fn attributes_for(&self, subject: &Subject, resource: &ResourceUrn) -> AttributeMap;
}

#[derive(Default)]
pub struct StaticAttributeProvider {
    pub base: AttributeMap,
}

#[async_trait]
impl AttributeProvider for StaticAttributeProvider {
    async fn attributes_for(&self, _subject: &Subject, _resource: &ResourceUrn) -> AttributeMap {
        self.base.clone()
    }
}

pub fn attrs_from_map(map: HashMap<String, Value>) -> AttributeMap {
    AttributeMap(Value::Object(map.into_iter().collect()))
}
