use serde_json::json;

use crate::{
    ContentBlock,
    runtime::{
        Agent, AgentEvent, ContextCompactionTrigger, SpawnedAgentStatus, execution_context, task,
        team,
    },
    tool::{ToolCall, ToolSpec},
};

pub(crate) const COMPACT_TOOL_NAME: &str = "compact";
pub(crate) const IDLE_TOOL_NAME: &str = "idle";
pub(crate) const TASK_TOOL_NAME: &str = "task";

pub(crate) struct IntrinsicOutcome {
    pub(crate) result: ContentBlock,
    pub(crate) touched_task: bool,
    pub(crate) end_turn: bool,
}

pub(crate) fn specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: COMPACT_TOOL_NAME.to_string(),
            description: Some("Compress older conversation context into a summary.".into()),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolSpec {
            name: IDLE_TOOL_NAME.to_string(),
            description: Some(
                "Yield the current turn and return to the teammate idle loop.".into(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolSpec {
            name: TASK_TOOL_NAME.to_string(),
            description: Some(
                "Spawn a fresh subagent to work a subtask and return a concise summary.".into(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "Delegated task prompt for the subagent"
                    }
                },
                "required": ["prompt"]
            }),
        },
    ]
    .into_iter()
    .chain(execution_context::intrinsic_specs())
    .chain(task::intrinsic_specs())
    .chain(team::intrinsic_specs())
    .collect()
}

pub(crate) async fn execute(agent: &mut Agent, call: ToolCall) -> Option<IntrinsicOutcome> {
    match call.name.as_str() {
        COMPACT_TOOL_NAME => Some(IntrinsicOutcome {
            result: execute_compact(agent, call).await,
            touched_task: false,
            end_turn: false,
        }),
        IDLE_TOOL_NAME => Some(IntrinsicOutcome {
            result: execute_idle(agent, call),
            touched_task: false,
            end_turn: true,
        }),
        TASK_TOOL_NAME => Some(IntrinsicOutcome {
            result: execute_task(agent, call).await,
            touched_task: false,
            end_turn: false,
        }),
        name if team::is_team_tool(name) => {
            team::execute_intrinsic(agent, call)
                .await
                .map(|result| IntrinsicOutcome {
                    result,
                    touched_task: false,
                    end_turn: false,
                })
        }
        name if execution_context::is_execution_context_tool(name) => {
            execution_context::execute_intrinsic(agent, call).map(|result| IntrinsicOutcome {
                result: result.result,
                touched_task: result.touched_task,
                end_turn: false,
            })
        }
        name if task::is_task_tool(name) => {
            task::execute_intrinsic(agent, call).map(|outcome| IntrinsicOutcome {
                result: outcome.result,
                touched_task: outcome.touched_task,
                end_turn: false,
            })
        }
        _ => None,
    }
}

fn execute_idle(agent: &mut Agent, call: ToolCall) -> ContentBlock {
    agent.request_idle();
    ContentBlock::ToolResult {
        tool_use_id: call.id,
        content: "Yielding to the teammate idle loop.".to_string(),
        is_error: false,
    }
}

async fn execute_compact(agent: &mut Agent, call: ToolCall) -> ContentBlock {
    match agent
        .compact_history(
            agent.history().len().saturating_sub(1),
            ContextCompactionTrigger::Manual,
        )
        .await
    {
        Ok(Some(details)) => ContentBlock::ToolResult {
            tool_use_id: call.id,
            content: format!(
                "Context compacted. Transcript saved to {}",
                details.transcript_path.display()
            ),
            is_error: false,
        },
        Ok(None) => ContentBlock::ToolResult {
            tool_use_id: call.id,
            content: "Context compaction skipped because there was no older history to summarize."
                .to_string(),
            is_error: false,
        },
        Err(error) => ContentBlock::ToolResult {
            tool_use_id: call.id,
            content: format!("Context compaction failed: {error:?}"),
            is_error: true,
        },
    }
}

async fn execute_task(agent: &mut Agent, call: ToolCall) -> ContentBlock {
    match crate::runtime::agent::parse_task_input(call.input) {
        Ok(prompt) => {
            let mut child = match agent.spawn_subagent() {
                Ok(child) => child,
                Err(error) => {
                    return ContentBlock::ToolResult {
                        tool_use_id: call.id,
                        content: format!("Failed to spawn subagent: {error:?}"),
                        is_error: true,
                    };
                }
            };
            let started = agent.register_subagent(&child);
            agent.emit_event(AgentEvent::SubagentSpawned { agent: started });

            match Box::pin(child.send(vec![ContentBlock::Text { text: prompt }])).await {
                Ok(()) => {
                    if let Some(finished) =
                        agent.finish_subagent(child.id(), SpawnedAgentStatus::Finished)
                    {
                        agent.emit_event(AgentEvent::SubagentFinished { agent: finished });
                    }
                    if let Err(error) = agent.refresh_tasks_from_disk() {
                        return ContentBlock::ToolResult {
                            tool_use_id: call.id,
                            content: format!("Task refresh failed: {error:?}"),
                            is_error: true,
                        };
                    }

                    ContentBlock::ToolResult {
                        tool_use_id: call.id,
                        content: child.final_text_summary(),
                        is_error: false,
                    }
                }
                Err(error) => {
                    if let Some(finished) = agent.finish_subagent(
                        child.id(),
                        SpawnedAgentStatus::Failed(format!("{error:?}")),
                    ) {
                        agent.emit_event(AgentEvent::SubagentFinished { agent: finished });
                    }
                    let _ = agent.refresh_tasks_from_disk();

                    ContentBlock::ToolResult {
                        tool_use_id: call.id,
                        content: format!("Subagent failed: {error:?}"),
                        is_error: true,
                    }
                }
            }
        }
        Err(content) => ContentBlock::ToolResult {
            tool_use_id: call.id,
            content,
            is_error: true,
        },
    }
}
