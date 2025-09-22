use std::collections::BTreeMap;

use crate::model::{Action, ResourceUrn};

pub fn decision_labels(
    resource: &ResourceUrn,
    action: &Action,
    outcome: &str,
) -> BTreeMap<&'static str, String> {
    let mut map = BTreeMap::new();
    map.insert("resource", resource.0.clone());
    map.insert("action", format_action(action));
    map.insert("outcome", outcome.to_string());
    map
}

fn format_action(action: &Action) -> String {
    match action {
        Action::Read => "read",
        Action::Write => "write",
        Action::Invoke => "invoke",
        Action::List => "list",
        Action::Admin => "admin",
        Action::Configure => "configure",
    }
    .to_string()
}
