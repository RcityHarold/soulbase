use sb_errors::prelude::{labels, ErrorObj};
use std::collections::BTreeMap;

pub fn error_labels(
    err: &ErrorObj,
    resource: Option<&str>,
    action: Option<&str>,
) -> BTreeMap<&'static str, String> {
    let mut labels = labels(err);
    if let Some(res) = resource {
        labels.insert("resource", res.to_string());
    }
    if let Some(act) = action {
        labels.insert("action", act.to_string());
    }
    labels
}
