use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    provider::model::{
        ContentBlock, ContentBlockDelta, ContentBlockStart, Message, ModelProviderKind,
        ProviderError, ProviderEvent, Request, Role, ToolChoice,
    },
    runtime::{AgentConfig, AgentEvent, Runtime, SpawnedAgentStatus},
};

use super::support::{
    ScriptedProvider, StaticTool, StreamScript, erroring_stream, model_info, ok_stream,
};

#[tokio::test]
async fn send_tool_use_turn_executes_tool_and_commits_follow_up_response() {
    let model = model_info("model", ModelProviderKind::Anthropic);
    let provider = ScriptedProvider::new(
        ModelProviderKind::Anthropic,
        vec![model.clone()],
        vec![
            ok_stream(vec![
                ProviderEvent::MessageStarted {
                    id: "msg-1".to_string(),
                    model: model.id.clone(),
                    role: Role::Assistant,
                },
                ProviderEvent::ContentBlockStarted {
                    index: 0,
                    kind: ContentBlockStart::ToolUse {
                        id: "tool-1".to_string(),
                        name: "echo_tool".to_string(),
                    },
                },
                ProviderEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentBlockDelta::ToolUseInputJson(r#"{"value":"hi"}"#.to_string()),
                },
                ProviderEvent::ContentBlockStopped { index: 0 },
                ProviderEvent::MessageStopped,
            ]),
            ok_stream(vec![
                ProviderEvent::MessageStarted {
                    id: "msg-2".to_string(),
                    model: model.id.clone(),
                    role: Role::Assistant,
                },
                ProviderEvent::ContentBlockStarted {
                    index: 0,
                    kind: ContentBlockStart::Text,
                },
                ProviderEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentBlockDelta::Text("done".to_string()),
                },
                ProviderEvent::ContentBlockStopped { index: 0 },
                ProviderEvent::MessageStopped,
            ]),
        ],
    );

    let runtime = Runtime::empty_builder()
        .with_provider_instance(provider)
        .with_tool(StaticTool::success("echo_tool", "tool output"))
        .build()
        .expect("build runtime");
    let mut agent = runtime.spawn("agent", model).unwrap();
    let mut events = agent.subscribe_events();

    agent
        .send(vec![ContentBlock::Text {
            text: "hi".to_string(),
        }])
        .await
        .unwrap();

    assert_eq!(agent.history().len(), 4);
    assert_eq!(
        agent.history()[2],
        Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "tool-1".to_string(),
                content: "tool output".to_string(),
                is_error: false,
            }],
        }
    );
    assert_eq!(
        agent.last_message(),
        Some(&Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: "done".to_string(),
            }],
        })
    );

    let events = collect_events(&mut events);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentEvent::ToolUseReady { .. }))
    );
    assert!(events.iter().any(|event| matches!(
        event,
        AgentEvent::ToolExecutionFinished {
            result: ContentBlock::ToolResult {
                is_error: false,
                ..
            }
        }
    )));
}

#[tokio::test]
async fn tool_execution_error_is_wrapped_and_loop_continues() {
    let model = model_info("model", ModelProviderKind::Anthropic);
    let provider = ScriptedProvider::new(
        ModelProviderKind::Anthropic,
        vec![model.clone()],
        vec![
            ok_stream(vec![
                ProviderEvent::MessageStarted {
                    id: "msg-1".to_string(),
                    model: model.id.clone(),
                    role: Role::Assistant,
                },
                ProviderEvent::ContentBlockStarted {
                    index: 0,
                    kind: ContentBlockStart::ToolUse {
                        id: "tool-1".to_string(),
                        name: "failing_tool".to_string(),
                    },
                },
                ProviderEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentBlockDelta::ToolUseInputJson(r#"{"value":"hi"}"#.to_string()),
                },
                ProviderEvent::ContentBlockStopped { index: 0 },
                ProviderEvent::MessageStopped,
            ]),
            ok_stream(vec![
                ProviderEvent::MessageStarted {
                    id: "msg-2".to_string(),
                    model: model.id.clone(),
                    role: Role::Assistant,
                },
                ProviderEvent::ContentBlockStarted {
                    index: 0,
                    kind: ContentBlockStart::Text,
                },
                ProviderEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentBlockDelta::Text("handled".to_string()),
                },
                ProviderEvent::ContentBlockStopped { index: 0 },
                ProviderEvent::MessageStopped,
            ]),
        ],
    );

    let runtime = Runtime::empty_builder()
        .with_provider_instance(provider)
        .with_tool(StaticTool::failure("failing_tool", "tool failed"))
        .build()
        .expect("build runtime");
    let mut agent = runtime.spawn("agent", model).unwrap();

    agent
        .send(vec![ContentBlock::Text {
            text: "hi".to_string(),
        }])
        .await
        .unwrap();

    assert_eq!(
        agent.history()[2],
        Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "tool-1".to_string(),
                content: "tool failed".to_string(),
                is_error: true,
            }],
        }
    );
    assert_eq!(
        agent.last_message(),
        Some(&Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: "handled".to_string(),
            }],
        })
    );
}

#[tokio::test]
async fn default_runtime_exposes_task_and_new_empty_does_not() {
    let model = model_info("model", ModelProviderKind::Anthropic);

    let default_provider = ScriptedProvider::new(
        ModelProviderKind::Anthropic,
        vec![model.clone()],
        vec![text_stream(&model.id, "ok")],
    );
    let default_handle = default_provider.clone();
    let default_runtime = Runtime::builder()
        .with_provider_instance(default_provider)
        .build()
        .expect("build runtime");
    let mut default_agent = default_runtime.spawn("agent", model.clone()).unwrap();
    default_agent
        .send(vec![ContentBlock::Text {
            text: "hello".to_string(),
        }])
        .await
        .unwrap();

    let default_requests = default_handle.recorded_requests().await;
    let default_tools = tool_names(&default_requests[0]);
    assert!(default_tools.contains("bash"));
    assert!(default_tools.contains("compact"));
    assert!(default_tools.contains("read_file"));
    assert!(default_tools.contains("task"));
    assert!(default_tools.contains("task_create"));
    assert!(default_tools.contains("task_update"));
    assert!(default_tools.contains("task_list"));
    assert!(default_tools.contains("task_get"));
    assert!(!default_tools.contains("load_skill"));

    let empty_provider = ScriptedProvider::new(
        ModelProviderKind::Anthropic,
        vec![model.clone()],
        vec![text_stream(&model.id, "ok")],
    );
    let empty_handle = empty_provider.clone();
    let empty_runtime = Runtime::empty_builder()
        .with_provider_instance(empty_provider)
        .build()
        .expect("build runtime");
    let mut empty_agent = empty_runtime.spawn("agent", model).unwrap();
    empty_agent
        .send(vec![ContentBlock::Text {
            text: "hello".to_string(),
        }])
        .await
        .unwrap();

    let empty_requests = empty_handle.recorded_requests().await;
    let empty_tools = tool_names(&empty_requests[0]);
    assert!(!empty_tools.contains("compact"));
    assert!(!empty_tools.contains("task"));
    assert!(!empty_tools.contains("task_create"));
    assert!(!empty_tools.contains("task_update"));
    assert!(!empty_tools.contains("task_list"));
    assert!(!empty_tools.contains("task_get"));
    assert!(!empty_tools.contains("load_skill"));
}

#[tokio::test]
async fn registered_skills_are_exposed_and_load_skill_returns_wrapped_content() {
    let model = model_info("model", ModelProviderKind::Anthropic);
    let provider = ScriptedProvider::new(
        ModelProviderKind::Anthropic,
        vec![model.clone()],
        vec![
            tool_use_stream(&model.id, "tool-skill", "load_skill", r#"{"name":"git"}"#),
            text_stream(&model.id, "done"),
        ],
    );
    let provider_handle = provider.clone();

    let skills_dir = temp_skills_dir("load-skill");
    write_skill(
        &skills_dir,
        "git",
        "---\nname: git\ndescription: Git workflow helpers\n---\nUse feature branches.\nRun tests first.\n",
    );
    let runtime = Runtime::empty_builder()
        .with_provider_instance(provider)
        .with_skills_dir(&skills_dir)
        .expect("register skills")
        .build()
        .expect("build runtime");
    let mut agent = runtime
        .spawn_with_config(
            "agent",
            model,
            AgentConfig {
                system: Some("Base system prompt".to_string()),
                ..AgentConfig::default()
            },
        )
        .unwrap();

    agent
        .send(vec![ContentBlock::Text {
            text: "hello".to_string(),
        }])
        .await
        .unwrap();

    assert_eq!(
        agent.history()[2],
        Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "tool-skill".to_string(),
                content: "<skill name=\"git\">\nUse feature branches.\nRun tests first.\n</skill>"
                    .to_string(),
                is_error: false,
            }],
        }
    );

    let requests = provider_handle.recorded_requests().await;
    let tools = tool_names(&requests[0]);
    assert!(tools.contains("load_skill"));
    assert_eq!(
        requests[0].system.as_deref(),
        Some(
            "Base system prompt\n\nSkills available:\n  - git: Git workflow helpers\nUse the load_skill tool only when one of these skills is relevant to the task."
        )
    );
}

#[tokio::test]
async fn task_subagent_keeps_load_skill_while_hiding_task() {
    let model = model_info("model", ModelProviderKind::Anthropic);
    let provider = ScriptedProvider::new(
        ModelProviderKind::Anthropic,
        vec![model.clone()],
        vec![
            tool_use_stream(
                &model.id,
                "tool-parent",
                "task",
                r#"{"prompt":"inspect repo"}"#,
            ),
            text_stream(&model.id, "child summary"),
            text_stream(&model.id, "parent done"),
        ],
    );
    let provider_handle = provider.clone();

    let skills_dir = temp_skills_dir("subagent-skills");
    write_skill(
        &skills_dir,
        "review",
        "---\nname: review\ndescription: Code review checklist\n---\nCheck tests.\n",
    );
    let runtime = Runtime::builder()
        .with_provider_instance(provider)
        .with_skills_dir(&skills_dir)
        .expect("register skills")
        .build()
        .expect("build runtime");
    let mut agent = runtime.spawn("agent", model).unwrap();

    agent
        .send(vec![ContentBlock::Text {
            text: "delegate".to_string(),
        }])
        .await
        .unwrap();

    let requests = provider_handle.recorded_requests().await;
    let child_tools = tool_names(&requests[1]);
    assert!(child_tools.contains("load_skill"));
    assert!(!child_tools.contains("task"));
    assert_eq!(
        requests[1].system.as_deref(),
        Some(
            "You are a subagent working for another agent. Solve the delegated task, use tools when helpful, and finish with a concise final answer for the parent agent.\n\nSkills available:\n  - review: Code review checklist\nUse the load_skill tool only when one of these skills is relevant to the task."
        )
    );
}

#[tokio::test]
async fn task_tool_runs_child_with_isolated_history_and_filtered_tools() {
    let model = model_info("model", ModelProviderKind::Anthropic);
    let provider = ScriptedProvider::new(
        ModelProviderKind::Anthropic,
        vec![model.clone()],
        vec![
            tool_use_stream(
                &model.id,
                "tool-parent",
                "task",
                r#"{"prompt":"inspect repo"}"#,
            ),
            text_stream(&model.id, "child summary"),
            text_stream(&model.id, "parent done"),
        ],
    );
    let provider_handle = provider.clone();

    let runtime = Runtime::builder()
        .with_provider_instance(provider)
        .build()
        .expect("build runtime");
    let mut agent = runtime.spawn("agent", model.clone()).unwrap();
    let mut events = agent.subscribe_events();

    agent
        .send(vec![ContentBlock::Text {
            text: "delegate".to_string(),
        }])
        .await
        .unwrap();

    assert_eq!(agent.history().len(), 4);
    assert_eq!(
        agent.history()[2],
        Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "tool-parent".to_string(),
                content: "child summary".to_string(),
                is_error: false,
            }],
        }
    );
    assert_eq!(
        agent.last_message(),
        Some(&Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: "parent done".to_string(),
            }],
        })
    );

    let requests = provider_handle.recorded_requests().await;
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[1].messages.len(), 1);
    assert_eq!(requests[1].messages[0].role, Role::User);
    assert_eq!(
        requests[1].messages[0].content,
        vec![ContentBlock::Text {
            text: "inspect repo".to_string(),
        }]
    );

    let child_tools = tool_names(&requests[1]);
    assert!(child_tools.contains("bash"));
    assert!(child_tools.contains("read_file"));
    assert!(!child_tools.contains("task"));

    let subagents = agent.watch_snapshot().borrow().subagents.clone();
    assert_eq!(subagents.len(), 1);
    assert_eq!(subagents[0].name, "agent::task");
    assert_eq!(subagents[0].model, model.id);
    assert_eq!(subagents[0].status, SpawnedAgentStatus::Finished);

    let events = collect_events(&mut events);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentEvent::SubagentSpawned { .. }))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentEvent::SubagentFinished { .. }))
    );
}

#[tokio::test]
async fn task_subagent_does_not_force_hidden_task_tool_choice() {
    let model = model_info("model", ModelProviderKind::Anthropic);
    let provider = ScriptedProvider::new(
        ModelProviderKind::Anthropic,
        vec![model.clone()],
        vec![
            tool_use_stream(
                &model.id,
                "tool-parent",
                "task",
                r#"{"prompt":"inspect repo"}"#,
            ),
            text_stream(&model.id, "child summary"),
            text_stream(&model.id, "parent done"),
        ],
    );
    let provider_handle = provider.clone();

    let runtime = Runtime::builder()
        .with_provider_instance(provider)
        .build()
        .expect("build runtime");
    let mut agent = runtime
        .spawn_with_config(
            "agent",
            model,
            AgentConfig {
                tool_choice: Some(ToolChoice::Tool {
                    name: "task".to_string(),
                }),
                ..AgentConfig::default()
            },
        )
        .unwrap();

    agent
        .send(vec![ContentBlock::Text {
            text: "delegate".to_string(),
        }])
        .await
        .unwrap();

    let requests = provider_handle.recorded_requests().await;
    assert_eq!(
        requests[0].tool_choice,
        Some(ToolChoice::Tool {
            name: "task".to_string(),
        })
    );
    assert_eq!(requests[1].tool_choice, Some(ToolChoice::Auto));
    assert!(!tool_names(&requests[1]).contains("task"));
}

#[tokio::test]
async fn task_tool_wraps_child_failure_and_parent_continues() {
    let model = model_info("model", ModelProviderKind::Anthropic);
    let provider = ScriptedProvider::new(
        ModelProviderKind::Anthropic,
        vec![model.clone()],
        vec![
            tool_use_stream(
                &model.id,
                "tool-parent",
                "task",
                r#"{"prompt":"inspect repo"}"#,
            ),
            erroring_stream(
                vec![ProviderEvent::MessageStarted {
                    id: "child-msg".to_string(),
                    model: model.id.clone(),
                    role: Role::Assistant,
                }],
                ProviderError::MalformedStream("boom".to_string()),
            ),
            text_stream(&model.id, "handled"),
        ],
    );

    let runtime = Runtime::builder()
        .with_provider_instance(provider)
        .build()
        .expect("build runtime");
    let mut agent = runtime.spawn("agent", model).unwrap();

    agent
        .send(vec![ContentBlock::Text {
            text: "delegate".to_string(),
        }])
        .await
        .unwrap();

    assert_eq!(
        agent.history()[2],
        Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "tool-parent".to_string(),
                content: "Subagent failed: FailedToStreamResponse(MalformedStream(\"boom\"))"
                    .to_string(),
                is_error: true,
            }],
        }
    );
    assert_eq!(
        agent.last_message(),
        Some(&Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: "handled".to_string(),
            }],
        })
    );

    let subagents = agent.watch_snapshot().borrow().subagents.clone();
    assert_eq!(subagents.len(), 1);
    assert!(matches!(
        &subagents[0].status,
        SpawnedAgentStatus::Failed(message)
            if message == "FailedToStreamResponse(MalformedStream(\"boom\"))"
    ));
}

#[tokio::test]
async fn child_rejects_nested_task_requests_without_recursing() {
    let model = model_info("model", ModelProviderKind::Anthropic);
    let provider = ScriptedProvider::new(
        ModelProviderKind::Anthropic,
        vec![model.clone()],
        vec![
            tool_use_stream(&model.id, "parent-task", "task", r#"{"prompt":"delegate"}"#),
            tool_use_stream(&model.id, "child-task", "task", r#"{"prompt":"recurse"}"#),
            text_stream(&model.id, "child recovered"),
            text_stream(&model.id, "parent done"),
        ],
    );
    let provider_handle = provider.clone();

    let runtime = Runtime::builder()
        .with_provider_instance(provider)
        .build()
        .expect("build runtime");
    let mut agent = runtime.spawn("agent", model).unwrap();

    agent
        .send(vec![ContentBlock::Text {
            text: "delegate".to_string(),
        }])
        .await
        .unwrap();

    assert_eq!(
        agent.history()[2],
        Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "parent-task".to_string(),
                content: "child recovered".to_string(),
                is_error: false,
            }],
        }
    );

    let requests = provider_handle.recorded_requests().await;
    assert_eq!(requests.len(), 4);
    assert!(!tool_names(&requests[1]).contains("task"));
    assert_eq!(requests[2].messages.len(), 3);
    assert_eq!(
        requests[2].messages[2],
        Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "child-task".to_string(),
                content: "Tool 'task' is not available for this agent".to_string(),
                is_error: true,
            }],
        }
    );
}

#[tokio::test]
async fn task_tool_returns_error_when_child_hits_round_limit() {
    let model = model_info("model", ModelProviderKind::Anthropic);
    let mut scripts = vec![tool_use_stream(
        &model.id,
        "parent-task",
        "task",
        r#"{"prompt":"delegate"}"#,
    )];
    for index in 0..30 {
        scripts.push(tool_use_stream(
            &model.id,
            &format!("child-tool-{index}"),
            "echo_tool",
            r#"{"value":"ping"}"#,
        ));
    }
    scripts.push(text_stream(&model.id, "parent handled"));

    let provider =
        ScriptedProvider::new(ModelProviderKind::Anthropic, vec![model.clone()], scripts);
    let provider_handle = provider.clone();

    let runtime = Runtime::builder()
        .with_provider_instance(provider)
        .with_tool(StaticTool::success("echo_tool", "pong"))
        .build()
        .expect("build runtime");
    let mut agent = runtime.spawn("agent", model).unwrap();

    agent
        .send(vec![ContentBlock::Text {
            text: "delegate".to_string(),
        }])
        .await
        .unwrap();

    assert_eq!(
        agent.history()[2],
        Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "parent-task".to_string(),
                content: "Subagent failed: MaxRoundsExceeded(30)".to_string(),
                is_error: true,
            }],
        }
    );
    assert_eq!(
        agent.last_message(),
        Some(&Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: "parent handled".to_string(),
            }],
        })
    );

    let requests = provider_handle.recorded_requests().await;
    assert_eq!(requests.len(), 32);
}

fn collect_events(receiver: &mut tokio::sync::broadcast::Receiver<AgentEvent>) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    while let Ok(event) = receiver.try_recv() {
        events.push(event);
    }
    events
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

fn tool_names(request: &Request<'_>) -> std::collections::HashSet<String> {
    request.tools.iter().map(|tool| tool.name.clone()).collect()
}

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);

fn temp_skills_dir(label: &str) -> PathBuf {
    let unique = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "mentra-runtime-skills-{label}-{timestamp}-{unique}"
    ));
    fs::create_dir_all(&path).expect("create temp dir");
    path
}

fn write_skill(root: &Path, name: &str, content: &str) {
    let skill_dir = root.join(name);
    fs::create_dir_all(&skill_dir).expect("create skill dir");
    fs::write(skill_dir.join("SKILL.md"), content).expect("write skill");
}
