use std::collections::HashMap;
use std::collections::HashSet;

use futures_util::StreamExt;
use tokio::sync::mpsc;

use crate::{ProviderError, ProviderEvent, ProviderEventStream, TokenUsage};

use super::stream_model::AnthropicContentBlockDelta;
use super::stream_model::AnthropicStreamContentBlock;
use super::stream_model::AnthropicStreamEvent;

pub(crate) fn spawn_event_stream(response: reqwest::Response) -> ProviderEventStream {
    let (tx, rx) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        if let Err(error) = forward_events(response, tx.clone()).await {
            let _ = tx.send(Err(error));
        }
    });

    rx
}

async fn forward_events(
    response: reqwest::Response,
    tx: mpsc::UnboundedSender<Result<ProviderEvent, ProviderError>>,
) -> Result<(), ProviderError> {
    let mut bytes_stream = response.bytes_stream();
    let mut buffer = Vec::new();
    let mut state = StreamState::default();

    while let Some(chunk) = bytes_stream.next().await {
        let chunk = chunk.map_err(ProviderError::Transport)?;
        buffer.extend_from_slice(&chunk);

        while let Some((frame_end, delimiter_len)) = find_frame_boundary(&buffer) {
            let frame = buffer.drain(..frame_end).collect::<Vec<_>>();
            buffer.drain(..delimiter_len);

            for event in parse_frame(&frame, &mut state)? {
                if tx.send(Ok(event)).is_err() {
                    return Ok(());
                }
            }
        }
    }

    if !buffer.is_empty() {
        for event in parse_frame(&buffer, &mut state)? {
            let _ = tx.send(Ok(event));
        }
    }

    Ok(())
}

#[derive(Default)]
struct StreamState {
    ignored_blocks: HashSet<usize>,
    latest_usage: Option<TokenUsage>,
    block_kinds: HashMap<usize, StreamingBlockKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamingBlockKind {
    ToolUse,
    HostedToolSearch,
}

fn parse_frame(frame: &[u8], state: &mut StreamState) -> Result<Vec<ProviderEvent>, ProviderError> {
    let frame = std::str::from_utf8(frame)
        .map_err(|error| ProviderError::MalformedStream(error.to_string()))?;
    let mut data_lines = Vec::new();

    for raw_line in frame.lines() {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if line.is_empty() || line.starts_with(':') {
            continue;
        }

        if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.trim_start().to_string());
        }
    }

    if data_lines.is_empty() {
        return Ok(Vec::new());
    }

    let data = data_lines.join("\n");
    let event: AnthropicStreamEvent =
        serde_json::from_str(&data).map_err(ProviderError::Deserialize)?;

    if let AnthropicStreamEvent::ContentBlockStart {
        index,
        content_block,
    } = &event
    {
        match content_block {
            AnthropicStreamContentBlock::ToolUse { .. } => {
                state
                    .block_kinds
                    .insert(*index, StreamingBlockKind::ToolUse);
            }
            AnthropicStreamContentBlock::ServerToolUse { name, .. }
                if name.starts_with("tool_search") =>
            {
                state
                    .block_kinds
                    .insert(*index, StreamingBlockKind::HostedToolSearch);
            }
            _ => {}
        }
    }

    match &event {
        AnthropicStreamEvent::ContentBlockStart {
            index,
            content_block,
        } if !content_block.is_supported() => {
            state.ignored_blocks.insert(*index);
            return Ok(Vec::new());
        }
        AnthropicStreamEvent::ContentBlockDelta { index, .. }
        | AnthropicStreamEvent::ContentBlockStop { index }
            if state.ignored_blocks.contains(index) =>
        {
            if matches!(event, AnthropicStreamEvent::ContentBlockStop { .. }) {
                state.ignored_blocks.remove(index);
            }
            return Ok(Vec::new());
        }
        AnthropicStreamEvent::ContentBlockDelta {
            index,
            delta: AnthropicContentBlockDelta::InputJsonDelta { partial_json },
        } => {
            if matches!(
                state.block_kinds.get(index),
                Some(StreamingBlockKind::HostedToolSearch)
            ) {
                return Ok(vec![ProviderEvent::ContentBlockDelta {
                    index: *index,
                    delta: ProviderEventDeltaExt::hosted_tool_search_delta(partial_json),
                }]);
            }
        }
        _ => {}
    }

    let events = event.into_provider_events().map_err(|error| {
        ProviderError::MalformedStream(format!(
            "anthropic stream error ({}): {}",
            error.kind, error.message
        ))
    })?;

    Ok(events
        .into_iter()
        .map(|event| match event {
            ProviderEvent::ContentBlockStopped { index } => {
                state.block_kinds.remove(&index);
                ProviderEvent::ContentBlockStopped { index }
            }
            ProviderEvent::MessageDelta { stop_reason, usage } => {
                let usage = merge_usage(state.latest_usage.clone(), usage);
                state.latest_usage = usage.clone();
                ProviderEvent::MessageDelta { stop_reason, usage }
            }
            other => other,
        })
        .collect())
}

struct ProviderEventDeltaExt;

impl ProviderEventDeltaExt {
    fn hosted_tool_search_delta(partial_json: &str) -> crate::ContentBlockDelta {
        crate::ContentBlockDelta::HostedToolSearchQuery(
            extract_tool_search_query(partial_json).unwrap_or_else(|| partial_json.to_string()),
        )
    }
}

fn extract_tool_search_query(partial_json: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(partial_json)
        .ok()
        .and_then(|value| {
            value
                .get("query")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
}

fn merge_usage(base: Option<TokenUsage>, update: Option<TokenUsage>) -> Option<TokenUsage> {
    match (base, update) {
        (Some(base), Some(update)) => {
            let merged = TokenUsage {
                input_tokens: update.input_tokens.or(base.input_tokens),
                output_tokens: update.output_tokens.or(base.output_tokens),
                total_tokens: update.total_tokens.or(base.total_tokens),
                cache_read_input_tokens: update
                    .cache_read_input_tokens
                    .or(base.cache_read_input_tokens),
                cache_creation_input_tokens: update
                    .cache_creation_input_tokens
                    .or(base.cache_creation_input_tokens),
                reasoning_tokens: update.reasoning_tokens.or(base.reasoning_tokens),
                thoughts_tokens: update.thoughts_tokens.or(base.thoughts_tokens),
                tool_input_tokens: update.tool_input_tokens.or(base.tool_input_tokens),
            };
            Some(merged)
        }
        (Some(base), None) => Some(base),
        (None, Some(update)) => Some(update),
        (None, None) => None,
    }
}

fn find_frame_boundary(buffer: &[u8]) -> Option<(usize, usize)> {
    for (index, window) in buffer.windows(2).enumerate() {
        if window == b"\n\n" {
            return Some((index, 2));
        }
    }

    for (index, window) in buffer.windows(4).enumerate() {
        if window == b"\r\n\r\n" {
            return Some((index, 4));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{StreamState, parse_frame};
    use crate::{ProviderEvent, Role, TokenUsage};

    #[test]
    fn merges_anthropic_usage_updates_into_cumulative_totals() {
        let mut state = StreamState::default();

        let started = parse_frame(
            br#"data: {"type":"message_start","message":{"id":"msg_1","model":"claude-sonnet","role":"assistant","content":[],"usage":{"input_tokens":10,"cache_read_input_tokens":2}}}"#,
            &mut state,
        )
        .expect("message start should parse");
        assert_eq!(
            started,
            vec![
                ProviderEvent::MessageStarted {
                    id: "msg_1".to_string(),
                    model: "claude-sonnet".to_string(),
                    role: Role::Assistant,
                },
                ProviderEvent::MessageDelta {
                    stop_reason: None,
                    usage: Some(TokenUsage {
                        input_tokens: Some(10),
                        output_tokens: None,
                        total_tokens: None,
                        cache_read_input_tokens: Some(2),
                        cache_creation_input_tokens: None,
                        reasoning_tokens: None,
                        thoughts_tokens: None,
                        tool_input_tokens: None,
                    }),
                },
            ]
        );

        let delta = parse_frame(
            br#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn","usage":{"output_tokens":3}}}"#,
            &mut state,
        )
        .expect("message delta should parse");
        assert_eq!(
            delta,
            vec![ProviderEvent::MessageDelta {
                stop_reason: Some("end_turn".to_string()),
                usage: Some(TokenUsage {
                    input_tokens: Some(10),
                    output_tokens: Some(3),
                    total_tokens: None,
                    cache_read_input_tokens: Some(2),
                    cache_creation_input_tokens: None,
                    reasoning_tokens: None,
                    thoughts_tokens: None,
                    tool_input_tokens: None,
                }),
            }]
        );
    }

    #[test]
    fn parses_hosted_tool_search_bookkeeping_blocks() {
        let mut state = StreamState::default();

        let started = parse_frame(
            br#"data: {"type":"content_block_start","index":1,"content_block":{"type":"server_tool_use","id":"srvtoolu_1","name":"tool_search_tool_bm25"}}"#,
            &mut state,
        )
        .expect("server tool use should parse");
        assert_eq!(
            started,
            vec![ProviderEvent::ContentBlockStarted {
                index: 1,
                kind: crate::ContentBlockStart::HostedToolSearch {
                    call: crate::HostedToolSearchCall {
                        id: "srvtoolu_1".to_string(),
                        status: Some("in_progress".to_string()),
                        query: None,
                    },
                },
            }]
        );

        let delta = parse_frame(
            br#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"query\":\"weather\"}"}}"#,
            &mut state,
        )
        .expect("hosted search delta should parse");
        assert_eq!(
            delta,
            vec![ProviderEvent::ContentBlockDelta {
                index: 1,
                delta: crate::ContentBlockDelta::HostedToolSearchQuery("weather".to_string()),
            }]
        );

        let stopped = parse_frame(
            br#"data: {"type":"content_block_stop","index":1}"#,
            &mut state,
        )
        .expect("hosted search stop should parse");
        assert_eq!(
            stopped,
            vec![ProviderEvent::ContentBlockStopped { index: 1 }]
        );
    }
}
