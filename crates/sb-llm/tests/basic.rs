use futures_util::StreamExt;
use sb_llm::prelude::*;

fn registry_with_local() -> Registry {
    let mut reg = Registry::new();
    LocalProviderFactory::install(&mut reg);
    reg
}

fn user_message(text: &str) -> Message {
    Message {
        role: Role::User,
        segments: vec![ContentSegment::Text {
            text: text.to_string(),
        }],
        tool_calls: Vec::new(),
    }
}

#[tokio::test]
async fn chat_sync_and_stream_consistency() {
    let reg = registry_with_local();
    let chat = reg.chat("local:echo").expect("chat model");

    let req = ChatRequest {
        model_id: "local:echo".to_string(),
        messages: vec![
            Message {
                role: Role::System,
                segments: vec![ContentSegment::Text {
                    text: "You are echo.".into(),
                }],
                tool_calls: Vec::new(),
            },
            user_message("hello"),
        ],
        tool_specs: vec![],
        temperature: None,
        top_p: None,
        max_tokens: None,
        stop: Vec::new(),
        seed: None,
        frequency_penalty: None,
        presence_penalty: None,
        logit_bias: serde_json::Map::new(),
        response_format: None,
        idempotency_key: None,
        allow_sensitive: false,
        metadata: serde_json::Value::Null,
    };

    let sync = chat
        .chat(req.clone(), &StructOutPolicy::Off)
        .await
        .expect("chat response");
    let sync_text = match &sync.message.segments[0] {
        ContentSegment::Text { text } => text.clone(),
        _ => panic!("expected text segment"),
    };
    assert!(sync_text.starts_with("echo: "));

    let mut stream = chat
        .chat_stream(req, &StructOutPolicy::Off)
        .await
        .expect("stream");
    let mut concat = String::new();
    while let Some(delta) = stream.next().await {
        let delta = delta.expect("delta ok");
        if let Some(piece) = delta.text_delta {
            concat.push_str(&piece);
        }
    }
    assert_eq!(concat, sync_text);
}

#[tokio::test]
async fn chat_json_enforcement() {
    let reg = registry_with_local();
    let chat = reg.chat("local:echo").expect("chat model");

    let req = ChatRequest {
        model_id: "local:echo".into(),
        messages: vec![user_message("hi")],
        tool_specs: vec![],
        temperature: None,
        top_p: None,
        max_tokens: None,
        stop: Vec::new(),
        seed: None,
        frequency_penalty: None,
        presence_penalty: None,
        logit_bias: serde_json::Map::new(),
        response_format: Some(ResponseFormat {
            kind: ResponseKind::Json,
            json_schema: None,
            strict: true,
        }),
        idempotency_key: None,
        allow_sensitive: false,
        metadata: serde_json::Value::Null,
    };

    let resp = chat
        .chat(req, &StructOutPolicy::StrictReject)
        .await
        .expect("json response");
    let body = match &resp.message.segments[0] {
        ContentSegment::Text { text } => text.clone(),
        _ => panic!("expected text"),
    };
    let value: serde_json::Value = serde_json::from_str(&body).expect("valid json");
    assert_eq!(value["echo"], "hi");
}

#[tokio::test]
async fn embeddings_and_rerank_work() {
    let reg = registry_with_local();
    let embed = reg.embed("local:emb").expect("embed model");
    let response = embed
        .embed(EmbedRequest {
            model_id: "local:emb".into(),
            items: vec![
                EmbedItem {
                    id: "a".into(),
                    text: "the cat sat".into(),
                },
                EmbedItem {
                    id: "b".into(),
                    text: "cat on mat".into(),
                },
            ],
            normalize: true,
            pooling: None,
        })
        .await
        .expect("embed response");

    assert_eq!(response.dim, 8);
    assert_eq!(response.vectors.len(), 2);

    let rerank = reg.rerank("local:rerank").expect("rerank model");
    let rerank_resp = rerank
        .rerank(RerankRequest {
            model_id: "local:rerank".into(),
            query: "cat mat".into(),
            candidates: vec!["the cat sat".into(), "cat on mat".into()],
        })
        .await
        .expect("rerank response");

    assert_eq!(rerank_resp.ordering[0], 1);
    assert!(rerank_resp.scores[0] >= rerank_resp.scores[1]);
}
