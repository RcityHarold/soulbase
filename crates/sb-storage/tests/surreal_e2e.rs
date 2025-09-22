#![cfg(feature = "surreal")]

use sb_storage::model::Entity;
use sb_storage::prelude::{Graph, NamedArgs, Repository, Search, StorageResult, VectorIndex};
use sb_storage::surreal::config::{SurrealConfig, SurrealCredentials, SurrealProtocol};
use sb_storage::surreal::datastore::SurrealDatastore;
use sb_storage::surreal::graph::SurrealGraph;
use sb_storage::surreal::repo::SurrealRepository;
use sb_storage::surreal::search::SurrealSearch;
use sb_storage::surreal::vector::SurrealVectorIndex;
use sb_types::prelude::TenantId;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::env;

const TEST_TENANT: &str = "sb-storage-test";

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Doc {
    pub id: String,
    pub tenant: String,
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub vector: Option<Vec<f32>>,
}

impl Entity for Doc {
    const TABLE: &'static str = "doc";
    type Key = String;

    fn id(&self) -> &str {
        &self.id
    }
}

fn load_test_config() -> Option<SurrealConfig> {
    let endpoint = env::var("SURREAL_URL").ok()?;
    let namespace = env::var("SURREAL_NAMESPACE").unwrap_or_else(|_| "soul".into());
    let database = env::var("SURREAL_DATABASE").unwrap_or_else(|_| "base".into());
    let protocol = if endpoint.starts_with("http") {
        SurrealProtocol::Http
    } else {
        SurrealProtocol::Ws
    };

    let mut config = SurrealConfig {
        endpoint,
        namespace,
        database,
        protocol,
        credentials: None,
        max_connections: 4,
        strict: true,
    };

    if let (Ok(username), Ok(password)) =
        (env::var("SURREAL_USERNAME"), env::var("SURREAL_PASSWORD"))
    {
        config = config.with_credentials(SurrealCredentials::new(username, password));
    }

    Some(config)
}

async fn ensure_schema(config: &SurrealConfig) -> StorageResult<()> {
    let datastore = SurrealDatastore::connect(config.clone()).await?;
    let pool = datastore.pool();
    let args = NamedArgs::default();

    let _ = pool.run_raw("REMOVE TABLE doc", &args).await;
    let _ = pool.run_raw("REMOVE TABLE likes", &args).await;

    pool.run_raw(
        "DEFINE TABLE doc SCHEMALESS;
         DEFINE FIELD tenant ON doc TYPE string;
         DEFINE FIELD title ON doc TYPE string;
         DEFINE FIELD content ON doc TYPE string;
         DEFINE FIELD vector ON doc TYPE array;
         DEFINE INDEX doc_title_search ON TABLE doc FIELDS title SEARCH ANALYZER simple;
         DEFINE INDEX doc_vector_idx ON TABLE doc FIELDS vector VECTOR(3);
         DEFINE TABLE likes SCHEMALESS;
         DEFINE FIELD tenant ON likes TYPE string;",
        &args,
    )
    .await?;

    pool.run_raw(
        &format!(
            "DELETE doc WHERE tenant = '{tenant}'; DELETE likes WHERE tenant = '{tenant}';",
            tenant = TEST_TENANT
        ),
        &args,
    )
    .await?;

    Ok(())
}

#[tokio::test]
async fn surreal_end_to_end_smoke() -> StorageResult<()> {
    let Some(config) = load_test_config() else {
        eprintln!("skipping surreal_end_to_end_smoke: SURREAL_URL not set");
        return Ok(());
    };

    ensure_schema(&config).await?;

    let tenant = TenantId::from(TEST_TENANT);

    // Repository CRUD ------------------------------------------------------
    let repo_ds = SurrealDatastore::connect(config.clone()).await?;
    let repo = SurrealRepository::<Doc>::new(repo_ds);

    let doc1_id = format!("doc:{}_1", TEST_TENANT);
    let doc2_id = format!("doc:{}_2", TEST_TENANT);

    let doc1 = Doc {
        id: doc1_id.clone(),
        tenant: TEST_TENANT.into(),
        title: "alpha record".into(),
        content: "first document".into(),
        vector: None,
    };
    let doc2 = Doc {
        id: doc2_id.clone(),
        tenant: TEST_TENANT.into(),
        title: "beta record".into(),
        content: "second document".into(),
        vector: None,
    };

    let created1 = repo.create(&tenant, &doc1).await?;
    assert_eq!(created1.id(), doc1_id);
    repo.create(&tenant, &doc2).await?;

    let fetched = repo.get(&tenant, &doc1_id).await?.expect("doc1 fetched");
    assert_eq!(fetched.title, "alpha record");

    let patched = repo
        .upsert(
            &tenant,
            &doc1_id,
            json!({"title": "alpha updated", "content": "first document updated"}),
            None,
        )
        .await?;
    assert_eq!(patched.title, "alpha updated");

    let page = repo
        .select(&tenant, json!({"tenant": TEST_TENANT}), None, 10, None)
        .await?;
    assert!(page.items.iter().any(|d| d.id() == doc1_id));

    // Graph relations ------------------------------------------------------
    let graph_ds = SurrealDatastore::connect(config.clone()).await?;
    let graph = SurrealGraph::new(graph_ds);
    graph
        .relate(&tenant, &doc1_id, "likes", &doc2_id, json!({"weight": 1}))
        .await?;
    let outs: Vec<Doc> = graph.out(&tenant, &doc1_id, "likes", 10).await?;
    assert!(outs.iter().any(|d| d.id() == doc2_id));

    // Vector index ---------------------------------------------------------
    let vector_ds = SurrealDatastore::connect(config.clone()).await?;
    let vector_index = SurrealVectorIndex::new(vector_ds);
    vector_index
        .upsert_vec(&tenant, &doc1_id, &[0.1, 0.4, 0.9])
        .await?;
    let neighbors = vector_index
        .knn::<Doc>(&tenant, &[0.1, 0.4, 0.9], 1, None)
        .await?;
    assert!(neighbors.iter().any(|(d, _)| d.id() == doc1_id));

    // Search ---------------------------------------------------------------
    let search_ds = SurrealDatastore::connect(config.clone()).await?;
    let search = SurrealSearch::new(search_ds);
    let results = search.search::<Doc>(&tenant, "alpha", 5, None).await?;
    assert!(results.items.iter().any(|d| d.id() == doc1_id));

    // Cleanup --------------------------------------------------------------
    repo.delete(&tenant, &doc1_id).await?;
    repo.delete(&tenant, &doc2_id).await?;

    Ok(())
}
