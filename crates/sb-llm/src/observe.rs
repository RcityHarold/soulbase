use std::collections::BTreeMap;

pub fn labels(provider: &str, model: &str, code: Option<&str>) -> BTreeMap<&'static str, String> {
    let mut labels = BTreeMap::new();
    labels.insert("provider", provider.to_string());
    labels.insert("model", model.to_string());
    if let Some(code) = code {
        labels.insert("code", code.to_string());
    }
    labels
}
