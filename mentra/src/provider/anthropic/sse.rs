use std::collections::HashSet;

use futures_util::StreamExt;
use tokio::sync::mpsc;

use crate::provider::model::{ProviderError, ProviderEvent, ProviderEventStream};

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
    let mut ignored_blocks = HashSet::new();

    while let Some(chunk) = bytes_stream.next().await {
        let chunk = chunk.map_err(ProviderError::Transport)?;
        buffer.extend_from_slice(&chunk);

        while let Some((frame_end, delimiter_len)) = find_frame_boundary(&buffer) {
            let frame = buffer.drain(..frame_end).collect::<Vec<_>>();
            buffer.drain(..delimiter_len);

            if let Some(event) = parse_frame(&frame, &mut ignored_blocks)?
                && tx.send(Ok(event)).is_err()
            {
                return Ok(());
            }
        }
    }

    if !buffer.is_empty()
        && let Some(event) = parse_frame(&buffer, &mut ignored_blocks)?
    {
        let _ = tx.send(Ok(event));
    }

    Ok(())
}

fn parse_frame(
    frame: &[u8],
    ignored_blocks: &mut HashSet<usize>,
) -> Result<Option<ProviderEvent>, ProviderError> {
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
        return Ok(None);
    }

    let data = data_lines.join("\n");
    let event: AnthropicStreamEvent =
        serde_json::from_str(&data).map_err(ProviderError::Deserialize)?;

    match &event {
        AnthropicStreamEvent::ContentBlockStart {
            index,
            content_block,
        } if !content_block.is_supported() => {
            ignored_blocks.insert(*index);
            return Ok(None);
        }
        AnthropicStreamEvent::ContentBlockDelta { index, .. }
        | AnthropicStreamEvent::ContentBlockStop { index }
            if ignored_blocks.contains(index) =>
        {
            if matches!(event, AnthropicStreamEvent::ContentBlockStop { .. }) {
                ignored_blocks.remove(index);
            }
            return Ok(None);
        }
        _ => {}
    }

    event.into_provider_event().map_err(|error| {
        ProviderError::MalformedStream(format!(
            "anthropic stream error ({}): {}",
            error.kind, error.message
        ))
    })
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
