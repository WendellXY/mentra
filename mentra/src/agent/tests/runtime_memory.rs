use std::{path::PathBuf, time::Duration};

use crate::{
    BuiltinProvider, ContentBlock, Message, Role,
    agent::{AgentConfig, ContextCompactionConfig, MemoryConfig},
    memory::{MemoryRecord, MemoryRecordKind, MemoryStore},
    provider::{ContentBlockDelta, ContentBlockStart, ProviderEvent},
    runtime::{HybridRuntimeStore, Runtime, SqliteRuntimeStore},
};

use super::support::{ScriptedProvider, StreamScript, model_info, ok_stream};

#[tokio::test]
async fn automatic_memory_search_injects_recalled_context_without_persisting_it() {
    let store = test_store("recalled-memory");
    store
        .upsert_records(&[MemoryRecord {
            record_id: "summary:agent:1".to_string(),
            agent_id: "agent-1".to_string(),
            kind: MemoryRecordKind::Summary,
            content: "The user prefers keeping memory automatic and bounded.".to_string(),
            source_revision: 1,
            created_at: 1,
            metadata_json: "{}".to_string(),
            source: Some("seed".to_string()),
            pinned: false,
            score: None,
        }])
        .expect("seed records");

    let model = model_info("model", BuiltinProvider::Anthropic);
    let provider = ScriptedProvider::new(
        BuiltinProvider::Anthropic,
        vec![model.clone()],
        vec![text_stream(&model.id, "done")],
    );
    let provider_handle = provider.clone();

    let runtime = Runtime::empty_builder()
        .with_store(store.clone())
        .with_provider_instance(provider)
        .build()
        .expect("build runtime");
    let mut agent = runtime.spawn("agent", model).expect("spawn agent");
    let agent_id = agent.id().to_string();
    store
        .upsert_records(&[MemoryRecord {
            record_id: format!("summary:{agent_id}:1"),
            agent_id: agent_id.clone(),
            kind: MemoryRecordKind::Summary,
            content: "The user prefers keeping memory automatic and bounded.".to_string(),
            source_revision: 1,
            created_at: 1,
            metadata_json: "{}".to_string(),
            source: Some("seed".to_string()),
            pinned: false,
            score: None,
        }])
        .expect("seed agent record");

    agent
        .send(vec![ContentBlock::Text {
            text: "Help me design memory".to_string(),
        }])
        .await
        .expect("run");

    let requests = provider_handle.recorded_requests().await;
    assert_eq!(requests.len(), 1);
    assert!(requests[0].messages.iter().any(|message| {
        message_text(message).contains("<recalled-memory>")
            && message_text(message).contains("memory automatic and bounded")
    }));
    assert!(
        agent
            .history()
            .iter()
            .all(|message| { !message_text(message).contains("<recalled-memory>") })
    );
}

#[tokio::test]
async fn successful_runs_are_ingested_and_searchable() {
    let store = test_store("memory-ingest");
    let model = model_info("model", BuiltinProvider::Anthropic);
    let provider = ScriptedProvider::new(
        BuiltinProvider::Anthropic,
        vec![model.clone()],
        vec![text_stream(&model.id, "finished task")],
    );

    let runtime = Runtime::empty_builder()
        .with_store(store.clone())
        .with_provider_instance(provider)
        .build()
        .expect("build runtime");
    let mut agent = runtime.spawn("agent", model).expect("spawn agent");
    let agent_id = agent.id().to_string();

    agent
        .send(vec![ContentBlock::Text {
            text: "remember this plan".to_string(),
        }])
        .await
        .expect("run");

    let records = wait_for_records(&store, &agent_id, "remember", 1).await;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].kind, MemoryRecordKind::Episode);
    assert!(records[0].content.contains("remember this plan"));
    assert!(records[0].content.contains("finished task"));
}

#[tokio::test]
async fn sqlite_memory_search_is_namespaced_per_agent() {
    let store = test_store("memory-isolation");
    store
        .upsert_records(&[
            MemoryRecord {
                record_id: "episode:a:1".to_string(),
                agent_id: "agent-a".to_string(),
                kind: MemoryRecordKind::Episode,
                content: "shared phrase alpha".to_string(),
                source_revision: 1,
                created_at: 1,
                metadata_json: "{}".to_string(),
                source: Some("seed".to_string()),
                pinned: false,
                score: None,
            },
            MemoryRecord {
                record_id: "episode:b:1".to_string(),
                agent_id: "agent-b".to_string(),
                kind: MemoryRecordKind::Episode,
                content: "shared phrase alpha".to_string(),
                source_revision: 1,
                created_at: 1,
                metadata_json: "{}".to_string(),
                source: Some("seed".to_string()),
                pinned: false,
                score: None,
            },
        ])
        .expect("seed records");

    let agent_a = store
        .search_records("agent-a", "shared alpha", 10)
        .expect("search agent a");
    let agent_b = store
        .search_records("agent-b", "shared alpha", 10)
        .expect("search agent b");

    assert_eq!(agent_a.len(), 1);
    assert_eq!(agent_b.len(), 1);
    assert_eq!(agent_a[0].agent_id, "agent-a");
    assert_eq!(agent_b[0].agent_id, "agent-b");
}

#[tokio::test]
async fn compacted_summaries_are_searchable() {
    let store = test_store("memory-compact");
    let model = model_info("model", BuiltinProvider::Anthropic);
    let provider = ScriptedProvider::new(
        BuiltinProvider::Anthropic,
        vec![model.clone()],
        vec![
            tool_use_stream(&model.id, "compact-1", "compact", "{}"),
            text_stream(&model.id, "summary about architecture"),
            text_stream(&model.id, "after compact"),
        ],
    );

    let runtime = Runtime::builder()
        .with_store(store.clone())
        .with_provider_instance(provider)
        .build()
        .expect("build runtime");
    let mut agent = runtime
        .spawn_with_config(
            "agent",
            model,
            AgentConfig {
                compaction: ContextCompactionConfig {
                    auto_compact_threshold_tokens: None,
                    transcript_dir: temp_dir("searchable-compact"),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .expect("spawn agent");
    let agent_id = agent.id().to_string();

    agent
        .send(vec![ContentBlock::Text {
            text: "please compact".to_string(),
        }])
        .await
        .expect("run");

    let records = store
        .search_records(&agent_id, "architecture", 10)
        .expect("search summaries");
    assert!(records.iter().any(|record| {
        record.kind == MemoryRecordKind::Summary
            && record.content.contains("summary about architecture")
    }));
}

#[tokio::test]
async fn hybrid_memory_recall_includes_provenance_in_hidden_context() {
    let store = hybrid_store("recalled-memory-provenance");
    let model = model_info("model", BuiltinProvider::Anthropic);
    store
        .upsert_records(&[MemoryRecord {
            record_id: "fact:agent:1".to_string(),
            agent_id: "agent-1".to_string(),
            kind: MemoryRecordKind::Fact,
            content: "The user prefers concise memory summaries.".to_string(),
            source_revision: 1,
            created_at: 1,
            metadata_json: "{}".to_string(),
            source: Some("manual_pin".to_string()),
            pinned: true,
            score: None,
        }])
        .expect("seed records");

    let provider = ScriptedProvider::new(
        BuiltinProvider::Anthropic,
        vec![model.clone()],
        vec![text_stream(&model.id, "done")],
    );
    let provider_handle = provider.clone();
    let runtime = Runtime::builder()
        .with_store(store.clone())
        .with_provider_instance(provider)
        .build()
        .expect("build runtime");
    let mut agent = runtime.spawn("agent", model).expect("spawn agent");
    let agent_id = agent.id().to_string();
    store
        .upsert_records(&[MemoryRecord {
            record_id: format!("fact:{agent_id}:1"),
            agent_id: agent_id.clone(),
            kind: MemoryRecordKind::Fact,
            content: "The user prefers concise memory summaries.".to_string(),
            source_revision: 1,
            created_at: 1,
            metadata_json: "{}".to_string(),
            source: Some("manual_pin".to_string()),
            pinned: true,
            score: None,
        }])
        .expect("seed agent record");

    agent
        .send(vec![ContentBlock::Text {
            text: "Design the memory flow".to_string(),
        }])
        .await
        .expect("run");

    let requests = provider_handle.recorded_requests().await;
    let injected = requests[0]
        .messages
        .iter()
        .find_map(|message| {
            let text = message_text(message);
            text.contains("<recalled-memory>")
                .then_some(text.to_string())
        })
        .expect("recalled memory");
    assert!(injected.contains("source=manual_pin"));
    assert!(injected.contains("why="));
}

#[tokio::test]
async fn memory_pin_tool_creates_searchable_hybrid_memory() {
    let store = hybrid_store("memory-pin-tool");
    let model = model_info("model", BuiltinProvider::Anthropic);
    let provider = ScriptedProvider::new(
        BuiltinProvider::Anthropic,
        vec![model.clone()],
        vec![
            tool_use_stream(
                &model.id,
                "memory-pin-1",
                "memory_pin",
                r#"{"content":"Remember that the user likes short answers."}"#,
            ),
            text_stream(&model.id, "pinned"),
        ],
    );
    let runtime = Runtime::builder()
        .with_store(store.clone())
        .with_provider_instance(provider)
        .build()
        .expect("build runtime");
    let mut agent = runtime
        .spawn_with_config(
            "agent",
            model,
            AgentConfig {
                memory: MemoryConfig {
                    write_tools_enabled: true,
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .expect("spawn agent");
    let agent_id = agent.id().to_string();

    agent
        .send(vec![ContentBlock::Text {
            text: "Please remember this.".to_string(),
        }])
        .await
        .expect("run");

    let records = wait_for_records(&store, &agent_id, "short answers", 1).await;
    assert!(records.iter().any(|record| {
        record.kind == MemoryRecordKind::Fact
            && record.pinned
            && record.source.as_deref() == Some("manual_pin")
    }));
}

#[tokio::test]
async fn memory_forget_tool_hides_pinned_hybrid_memory() {
    let store = hybrid_store("memory-forget-tool");
    let model = model_info("model", BuiltinProvider::Anthropic);
    let provider = ScriptedProvider::new(
        BuiltinProvider::Anthropic,
        vec![model.clone()],
        vec![
            tool_use_stream(
                &model.id,
                "memory-forget-1",
                "memory_forget",
                r#"{"record_id":"fact:forget:1"}"#,
            ),
            text_stream(&model.id, "forgot"),
        ],
    );
    let runtime = Runtime::builder()
        .with_store(store.clone())
        .with_provider_instance(provider)
        .build()
        .expect("build runtime");
    let mut agent = runtime
        .spawn_with_config(
            "agent",
            model,
            AgentConfig {
                memory: MemoryConfig {
                    write_tools_enabled: true,
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .expect("spawn agent");
    let agent_id = agent.id().to_string();
    store
        .upsert_records(&[MemoryRecord {
            record_id: "fact:forget:1".to_string(),
            agent_id: agent_id.clone(),
            kind: MemoryRecordKind::Fact,
            content: "The user likes short answers.".to_string(),
            source_revision: 1,
            created_at: 1,
            metadata_json: "{}".to_string(),
            source: Some("manual_pin".to_string()),
            pinned: true,
            score: None,
        }])
        .expect("seed records");

    agent
        .send(vec![ContentBlock::Text {
            text: "Forget that note.".to_string(),
        }])
        .await
        .expect("run");

    let records = store
        .search_records(&agent_id, "short answers", 10)
        .expect("search records");
    assert!(records.is_empty());
}

async fn wait_for_records(
    store: &impl MemoryStore,
    agent_id: &str,
    query: &str,
    expected: usize,
) -> Vec<MemoryRecord> {
    for _ in 0..50 {
        let records = store
            .search_records(agent_id, query, 10)
            .expect("search records");
        if records.len() >= expected {
            return records;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    store
        .search_records(agent_id, query, 10)
        .expect("final search")
}

fn test_store(prefix: &str) -> SqliteRuntimeStore {
    SqliteRuntimeStore::new(temp_dir(prefix).join("runtime.sqlite"))
}

fn hybrid_store(prefix: &str) -> HybridRuntimeStore {
    let dir = temp_dir(prefix);
    HybridRuntimeStore::with_memory_path(dir.join("runtime.sqlite"), dir.join("memory.sqlite"))
}

fn temp_dir(prefix: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    std::env::temp_dir().join(format!("mentra-{prefix}-{nanos}"))
}

fn text_stream(model: &str, text: &str) -> StreamScript {
    ok_stream(vec![
        ProviderEvent::MessageStarted {
            id: format!("msg-{text}"),
            model: model.to_string(),
            role: Role::Assistant,
        },
        ProviderEvent::ContentBlockStarted {
            index: 0,
            kind: ContentBlockStart::Text,
        },
        ProviderEvent::ContentBlockDelta {
            index: 0,
            delta: ContentBlockDelta::Text(text.to_string()),
        },
        ProviderEvent::ContentBlockStopped { index: 0 },
        ProviderEvent::MessageStopped,
    ])
}

fn tool_use_stream(model: &str, id: &str, name: &str, input_json: &str) -> StreamScript {
    ok_stream(vec![
        ProviderEvent::MessageStarted {
            id: format!("msg-{id}"),
            model: model.to_string(),
            role: Role::Assistant,
        },
        ProviderEvent::ContentBlockStarted {
            index: 0,
            kind: ContentBlockStart::ToolUse {
                id: id.to_string(),
                name: name.to_string(),
            },
        },
        ProviderEvent::ContentBlockDelta {
            index: 0,
            delta: ContentBlockDelta::ToolUseInputJson(input_json.to_string()),
        },
        ProviderEvent::ContentBlockStopped { index: 0 },
        ProviderEvent::MessageStopped,
    ])
}

fn message_text(message: &Message) -> &str {
    message
        .content
        .iter()
        .find_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .unwrap_or("")
}
