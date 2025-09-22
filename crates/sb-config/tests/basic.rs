use sb_config::prelude::*;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

#[test]
fn load_minimal_snapshot_and_read() {
    let registry = Arc::new(InMemorySchemaRegistry::new());
    registry
        .register_namespace(
            NamespaceId("app".into()),
            None,
            HashMap::from([(
                KeyPath("name".into()),
                FieldMeta {
                    reload: ReloadClass::HotReloadSafe,
                    sensitive: false,
                    default_value: Some(json!("Soulseed")),
                    description: None,
                },
            )]),
        )
        .expect("register schema");

    let loader = Loader {
        sources: vec![
            Arc::new(FileSource { paths: vec![] }),
            Arc::new(EnvSource {
                prefix: "SOUL".into(),
                separator: "__".into(),
            }),
        ],
        secrets: vec![Arc::new(NoopSecretResolver::default())],
        validator: Arc::new(BasicValidator::default()),
        schema_registry: registry.clone(),
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    let (snapshot, event) = rt.block_on(loader.load_with_prev(None)).expect("snapshot");

    let name: Option<String> = snapshot.get(&KeyPath("app.name".into()));
    assert_eq!(name.as_deref(), Some("Soulseed"));
    assert!(event.changed_keys.iter().any(|k| k.0 == "app.name"));

    let (_, event2) = rt
        .block_on(loader.load_with_prev(Some(&snapshot)))
        .expect("snapshot");
    assert!(event2.changed_keys.is_empty());
}
