use std::collections::BTreeMap;

pub type NamedArgs = BTreeMap<String, serde_json::Value>;

#[macro_export]
macro_rules! named {
    ( $( $key:expr => $value:expr ),* $(,)? ) => {{
        let mut map = ::std::collections::BTreeMap::new();
        $( map.insert($key.to_string(), ::serde_json::json!($value)); )*
        map
    }};
}
