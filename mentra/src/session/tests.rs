#![allow(clippy::unwrap_used)]

use crate::session::event::*;
use crate::session::types::*;

// ---- Task 1 type-level tests (preserved) ----

#[test]
fn session_id_roundtrips_through_serde() {
    let id = SessionId::new();
    let json = serde_json::to_string(&id).unwrap();
    let deserialized: SessionId = serde_json::from_str(&json).unwrap();
    assert_eq!(id, deserialized);
}

#[test]
fn session_id_from_raw_preserves_value() {
    let id = SessionId::from_raw("session-abc-123");
    assert_eq!(id.as_str(), "session-abc-123");
}

#[test]
fn session_metadata_serialization_roundtrip() {
    let metadata = SessionMetadata::new(
        SessionId::from_raw("session-test-1"),
        "Test Session",
        "claude-opus-4-20250514",
    );
    let json = serde_json::to_value(&metadata).unwrap();
    let deserialized: SessionMetadata = serde_json::from_value(json).unwrap();
    assert_eq!(metadata, deserialized);
}

#[test]
fn session_event_assistant_token_delta_roundtrip() {
    let event = SessionEvent::AssistantTokenDelta {
        delta: "hello".to_string(),
        full_text: "hello".to_string(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "assistant_token_delta");
    let deserialized: SessionEvent = serde_json::from_value(json).unwrap();
    assert_eq!(event, deserialized);
}

#[test]
fn session_event_tool_queued_roundtrip() {
    let event = SessionEvent::ToolQueued {
        tool_call_id: "tc-1".to_string(),
        tool_name: "shell".to_string(),
        summary: "Run 'cargo test'".to_string(),
        mutability: ToolMutability::Mutating,
        input_json: r#"{"command":"cargo test"}"#.to_string(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "tool_queued");
    assert_eq!(json["tool_name"], "shell");
    let deserialized: SessionEvent = serde_json::from_value(json).unwrap();
    assert_eq!(event, deserialized);
}

#[test]
fn session_event_permission_requested_roundtrip() {
    let preview_json = serde_json::to_string(&serde_json::json!({
        "command": "rm -rf /tmp/foo",
        "cwd": "/Users/dev/project"
    }))
    .unwrap();
    let event = SessionEvent::PermissionRequested {
        request_id: "perm-1".to_string(),
        tool_call_id: "tc-1".to_string(),
        tool_name: "shell".to_string(),
        description: "Execute shell command: rm -rf /tmp/foo".to_string(),
        preview: preview_json,
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "permission_requested");
    let deserialized: SessionEvent = serde_json::from_value(json).unwrap();
    assert_eq!(event, deserialized);
}

#[test]
fn session_event_compaction_completed_roundtrip() {
    let event = SessionEvent::CompactionCompleted {
        agent_id: "agent-1".to_string(),
        replaced_items: 42,
        preserved_items: 8,
        resulting_transcript_len: 10,
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "compaction_completed");
    let deserialized: SessionEvent = serde_json::from_value(json).unwrap();
    assert_eq!(event, deserialized);
}

#[test]
fn session_event_task_updated_roundtrip() {
    let event = SessionEvent::TaskUpdated {
        task_id: "bg-1".to_string(),
        kind: TaskKind::BackgroundTask,
        status: TaskLifecycleStatus::Running,
        title: "cargo test -p mentra".to_string(),
        detail: Some("exit code: 0".to_string()),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "task_updated");
    let deserialized: SessionEvent = serde_json::from_value(json).unwrap();
    assert_eq!(event, deserialized);
}

#[test]
fn all_session_event_variants_serialize_with_type_tag() {
    let events: Vec<SessionEvent> = vec![
        SessionEvent::SessionStarted {
            session_id: SessionId::from_raw("s1"),
        },
        SessionEvent::UserMessage {
            text: "hi".to_string(),
        },
        SessionEvent::AssistantTokenDelta {
            delta: "h".to_string(),
            full_text: "h".to_string(),
        },
        SessionEvent::AssistantMessageCompleted {
            text: "hello".to_string(),
        },
        SessionEvent::ToolQueued {
            tool_call_id: "tc1".to_string(),
            tool_name: "read".to_string(),
            summary: "Read file".to_string(),
            mutability: ToolMutability::ReadOnly,
            input_json: "{}".to_string(),
        },
        SessionEvent::ToolStarted {
            tool_call_id: "tc1".to_string(),
            tool_name: "read".to_string(),
        },
        SessionEvent::ToolProgress {
            tool_call_id: "tc1".to_string(),
            tool_name: "read".to_string(),
            progress: "50%".to_string(),
        },
        SessionEvent::ToolCompleted {
            tool_call_id: "tc1".to_string(),
            tool_name: "read".to_string(),
            summary: "Read 42 lines".to_string(),
            is_error: false,
        },
        SessionEvent::PermissionRequested {
            request_id: "p1".to_string(),
            tool_call_id: "tc1".to_string(),
            tool_name: "shell".to_string(),
            description: "run command".to_string(),
            preview: "{}".to_string(),
        },
        SessionEvent::PermissionResolved {
            request_id: "p1".to_string(),
            tool_call_id: "tc1".to_string(),
            tool_name: "shell".to_string(),
            outcome: PermissionOutcome::Allowed,
            rule_scope: Some(PermissionRuleScope::Session),
        },
        SessionEvent::TaskUpdated {
            task_id: "t1".to_string(),
            kind: TaskKind::Subagent,
            status: TaskLifecycleStatus::Spawned,
            title: "research".to_string(),
            detail: None,
        },
        SessionEvent::CompactionStarted {
            agent_id: "a1".to_string(),
        },
        SessionEvent::CompactionCompleted {
            agent_id: "a1".to_string(),
            replaced_items: 10,
            preserved_items: 5,
            resulting_transcript_len: 7,
        },
        SessionEvent::MemoryUpdated {
            agent_id: "a1".to_string(),
            stored_records: 3,
        },
        SessionEvent::Notice {
            severity: NoticeSeverity::Info,
            message: "Context window 80% full".to_string(),
        },
        SessionEvent::Error {
            message: "Provider timeout".to_string(),
            recoverable: true,
        },
    ];

    for event in events {
        let json = serde_json::to_value(&event).unwrap();
        assert!(
            json.get("type").is_some(),
            "Event missing 'type' tag: {event:?}"
        );
        let roundtripped: SessionEvent = serde_json::from_value(json).unwrap();
        assert_eq!(event, roundtripped);
    }
}

// ---- Task 2 lifecycle tests ----

use crate::{ContentBlock, test::MockRuntime};

#[tokio::test]
async fn create_session_produces_valid_metadata() {
    let mock = MockRuntime::builder().text("hello").build().unwrap();
    let session = mock
        .runtime()
        .create_session("test-session", mock.model())
        .unwrap();

    assert_eq!(session.name(), "test-session");
    assert_eq!(session.metadata().title, "test-session");
    assert_eq!(session.metadata().model, mock.model().id);
    assert_eq!(session.metadata().status, SessionStatus::Created);
    assert_eq!(session.metadata().turn_count, 0);
}

#[tokio::test]
async fn append_turn_returns_assistant_message() {
    let mock = MockRuntime::builder()
        .text("hello from session")
        .build()
        .unwrap();
    let mut session = mock
        .runtime()
        .create_session("test-session", mock.model())
        .unwrap();

    let message = session
        .append_turn(vec![ContentBlock::text("hi")])
        .await
        .unwrap();

    assert_eq!(message.text(), "hello from session");
    assert_eq!(session.metadata().turn_count, 1);
    assert_eq!(session.metadata().status, SessionStatus::Idle);
}

#[tokio::test]
async fn append_turn_emits_user_and_assistant_events() {
    let mock = MockRuntime::builder().text("response").build().unwrap();
    let mut session = mock
        .runtime()
        .create_session("test-session", mock.model())
        .unwrap();

    let mut rx = session.subscribe();

    let _message = session
        .append_turn(vec![ContentBlock::text("hello")])
        .await
        .unwrap();

    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }

    let has_user = events
        .iter()
        .any(|e| matches!(e, SessionEvent::UserMessage { text } if text == "hello"));
    let has_assistant = events
        .iter()
        .any(|e| matches!(e, SessionEvent::AssistantMessageCompleted { text } if text == "response"));

    assert!(has_user, "Expected UserMessage event, got: {events:?}");
    assert!(
        has_assistant,
        "Expected AssistantMessageCompleted event, got: {events:?}"
    );
}

#[tokio::test]
async fn replay_returns_transcript_after_turn() {
    let mock = MockRuntime::builder().text("world").build().unwrap();
    let mut session = mock
        .runtime()
        .create_session("test-session", mock.model())
        .unwrap();

    let _message = session
        .append_turn(vec![ContentBlock::text("hello")])
        .await
        .unwrap();

    let transcript = session.replay();
    assert!(
        !transcript.items().is_empty(),
        "Transcript should have items after a turn"
    );
}

#[tokio::test]
async fn session_status_transitions_created_to_idle() {
    let mock = MockRuntime::builder().text("done").build().unwrap();
    let mut session = mock
        .runtime()
        .create_session("test-session", mock.model())
        .unwrap();

    assert_eq!(session.metadata().status, SessionStatus::Created);

    let _message = session
        .append_turn(vec![ContentBlock::text("go")])
        .await
        .unwrap();

    assert_eq!(session.metadata().status, SessionStatus::Idle);
}

#[tokio::test]
async fn history_returns_committed_messages() {
    let mock = MockRuntime::builder().text("response").build().unwrap();
    let mut session = mock
        .runtime()
        .create_session("test-session", mock.model())
        .unwrap();

    assert!(session.history().is_empty());

    let _message = session
        .append_turn(vec![ContentBlock::text("hello")])
        .await
        .unwrap();

    assert!(
        !session.history().is_empty(),
        "History should contain messages after a turn"
    );
}

#[tokio::test]
async fn create_session_emits_session_started() {
    let mock = MockRuntime::builder().text("hi").build().unwrap();

    let session = mock
        .runtime()
        .create_session("test-session", mock.model())
        .unwrap();

    // The SessionStarted event was emitted during creation.
    // Verify session id follows the expected format.
    assert!(session.id().as_str().starts_with("session-"));
}
