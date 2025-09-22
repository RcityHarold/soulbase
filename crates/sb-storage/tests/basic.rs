use sb_storage::make_record_id;
use sb_storage::mock::{
    InMemoryGraph, InMemoryMigrator, InMemoryRepository, InMemorySearch, InMemoryVector,
    MockDatastore,
};
use sb_storage::model::Entity;
use sb_storage::prelude::*;
use sb_types::prelude::TenantId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct Doc {
    id: String,
    tenant: String,
    title: String,
    ver: u64,
}

impl Entity for Doc {
    const TABLE: &'static str = "doc";
    type Key = String;

    fn id(&self) -> &str {
        &self.id
    }
}

#[tokio::test]
async fn crud_and_select() {
    let ds = MockDatastore::new();
    let repo: InMemoryRepository<Doc> = InMemoryRepository::new(&ds);
    let tenant = TenantId("tenantA".into());

    let mut d1 = Doc {
        id: make_record_id(Doc::TABLE, &tenant, "001"),
        tenant: tenant.0.clone(),
        title: "hello".into(),
        ver: 1,
    };
    let d2 = Doc {
        id: make_record_id(Doc::TABLE, &tenant, "002"),
        tenant: tenant.0.clone(),
        title: "hi".into(),
        ver: 1,
    };

    d1 = repo.create(&tenant, &d1).await.unwrap();
    repo.create(&tenant, &d2).await.unwrap();

    let got = repo.get(&tenant, &d1.id).await.unwrap().unwrap();
    assert_eq!(got.title, "hello");

    let page = repo
        .select(
            &tenant,
            serde_json::json!({"tenant": tenant.0 }),
            None,
            10,
            None,
        )
        .await
        .unwrap();
    assert_eq!(page.items.len(), 2);

    let updated = repo
        .upsert(
            &tenant,
            &d1.id,
            serde_json::json!({"title": "hello2", "ver": 2}),
            Some(1),
        )
        .await
        .unwrap();
    assert_eq!(updated.title, "hello2");

    repo.delete(&tenant, &d2.id).await.unwrap();
    let page2 = repo
        .select(
            &tenant,
            serde_json::json!({"tenant": tenant.0 }),
            None,
            10,
            None,
        )
        .await
        .unwrap();
    assert_eq!(page2.items.len(), 1);
}

#[tokio::test]
async fn graph_and_vector() {
    let ds = MockDatastore::new();
    let repo: InMemoryRepository<Doc> = InMemoryRepository::new(&ds);
    let graph = InMemoryGraph::new(&ds);
    let vectors = InMemoryVector::new(&ds);
    let tenant = TenantId("tenantA".into());

    let a = Doc {
        id: make_record_id(Doc::TABLE, &tenant, "a"),
        tenant: tenant.0.clone(),
        title: "cat sat".into(),
        ver: 1,
    };
    let b = Doc {
        id: make_record_id(Doc::TABLE, &tenant, "b"),
        tenant: tenant.0.clone(),
        title: "cat on mat".into(),
        ver: 1,
    };
    repo.create(&tenant, &a).await.unwrap();
    repo.create(&tenant, &b).await.unwrap();

    graph
        .relate(
            &tenant,
            &a.id,
            "edge_like",
            &b.id,
            serde_json::json!({"at": 1}),
        )
        .await
        .unwrap();
    let outs: Vec<Doc> = graph.out(&tenant, &a.id, "edge_like", 10).await.unwrap();
    assert_eq!(outs.len(), 1);
    assert_eq!(outs[0].id, b.id);

    vectors
        .upsert_vec(&tenant, &a.id, &[1.0, 0.0, 0.0])
        .await
        .unwrap();
    vectors
        .upsert_vec(&tenant, &b.id, &[1.0, 0.1, 0.0])
        .await
        .unwrap();
    let hits = vectors
        .knn::<Doc>(&tenant, &[1.0, 0.05, 0.0], 1, None)
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].0.id, b.id);
}

#[tokio::test]
async fn tenant_isolation() {
    let ds = MockDatastore::new();
    let repo: InMemoryRepository<Doc> = InMemoryRepository::new(&ds);
    let t1 = TenantId("t1".into());
    let t2 = TenantId("t2".into());

    let doc = Doc {
        id: make_record_id(Doc::TABLE, &t1, "a"),
        tenant: t1.0.clone(),
        title: "A".into(),
        ver: 1,
    };
    repo.create(&t1, &doc).await.unwrap();

    let page = repo
        .select(&t2, serde_json::json!({"tenant": t2.0 }), None, 10, None)
        .await
        .unwrap();
    assert!(page.items.is_empty());
}

#[tokio::test]
async fn migration_tracks_versions() {
    let migrator = InMemoryMigrator::new();
    assert_eq!(migrator.current_version().await.unwrap(), "none");
    let scripts = vec![MigrationScript {
        version: "2025-09-12T15-30-00__init".into(),
        up_sql: "DEFINE TABLE doc SCHEMALESS;".into(),
        down_sql: "REMOVE TABLE doc;".into(),
        checksum: "sha256:abc".into(),
    }];
    migrator.apply_up(&scripts).await.unwrap();
    assert_eq!(
        migrator.current_version().await.unwrap(),
        "2025-09-12T15-30-00__init"
    );
}
#[tokio::test]
async fn fulltext_search() {
    let ds = MockDatastore::new();
    let repo: InMemoryRepository<Doc> = InMemoryRepository::new(&ds);
    let search = InMemorySearch::new(&ds);
    let tenant = TenantId("tenantA".into());

    let d1 = Doc {
        id: make_record_id(Doc::TABLE, &tenant, "001"),
        tenant: tenant.0.clone(),
        title: "rust async storage".into(),
        ver: 1,
    };
    let d2 = Doc {
        id: make_record_id(Doc::TABLE, &tenant, "002"),
        tenant: tenant.0.clone(),
        title: "graph vector index".into(),
        ver: 1,
    };
    repo.create(&tenant, &d1).await.unwrap();
    repo.create(&tenant, &d2).await.unwrap();

    let result = search
        .search::<Doc>(&tenant, "storage", 5, None)
        .await
        .unwrap();
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].id, d1.id);
}
