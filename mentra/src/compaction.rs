use std::{
    borrow::Cow,
    path::Path,
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;

use crate::{
    error::RuntimeError,
    provider::{
        CompactionInputItem, CompactionRequest as ProviderCompactionRequest,
        CompactionResponse as ProviderCompactionResponse, Provider, ProviderError,
        ProviderRequestOptions, Request,
    },
    transcript::{
        AgentTranscript, CompactionSummary, TranscriptItem, TranscriptKind,
    },
    ContentBlock, Message,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CompactionMode {
    #[default]
    LocalOnly,
    PreferRemote,
    RemoteOnly,
}

#[derive(Debug, Clone)]
pub struct CompactionRequest {
    pub model: String,
    pub transcript: AgentTranscript,
    pub transcript_dir: PathBuf,
    pub summary_max_input_chars: usize,
    pub summary_max_output_tokens: u32,
    pub preserve_recent_user_tokens: usize,
    pub preserve_recent_delegation_results: usize,
    pub provider_request_options: ProviderRequestOptions,
    pub mode: CompactionMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionExecutionMode {
    Local,
    Remote,
}

#[derive(Debug, Clone)]
pub struct CompactionOutcome {
    pub mode: CompactionExecutionMode,
    pub transcript_path: PathBuf,
    pub transcript: AgentTranscript,
    pub summary: CompactionSummary,
    pub replaced_items: usize,
    pub preserved_items: usize,
    pub preserved_user_turns: usize,
    pub preserved_delegation_results: usize,
}

#[async_trait]
pub trait CompactionEngine: Send + Sync {
    async fn compact(
        &self,
        provider: Arc<dyn Provider>,
        request: CompactionRequest,
    ) -> Result<Option<CompactionOutcome>, RuntimeError>;
}

#[derive(Debug, Default)]
pub struct StandardCompactionEngine;

#[async_trait]
impl CompactionEngine for StandardCompactionEngine {
    async fn compact(
        &self,
        provider: Arc<dyn Provider>,
        request: CompactionRequest,
    ) -> Result<Option<CompactionOutcome>, RuntimeError> {
        if request.transcript.is_empty() {
            return Ok(None);
        }

        let preserve_from = required_tail_start_for_continuation(request.transcript.items());
        if preserve_from == 0 {
            return Ok(None);
        }
        let compacted_prefix = &request.transcript.items()[..preserve_from];
        if compacted_prefix.is_empty() {
            return Ok(None);
        }

        let transcript_path = persist_transcript(request.transcript.items(), &request.transcript_dir).await?;
        let supports_remote = provider.capabilities().supports_history_compaction;
        let (mode, summary) = match request.mode {
            CompactionMode::LocalOnly => {
                (CompactionExecutionMode::Local, summarize_locally(provider, &request, compacted_prefix).await?)
            }
            CompactionMode::PreferRemote => {
                if supports_remote {
                    match compact_remotely(provider.clone(), &request, compacted_prefix).await {
                        Ok(Some(summary)) => (CompactionExecutionMode::Remote, summary),
                        Ok(None)
                        | Err(RuntimeError::FailedToCompactHistory(
                            ProviderError::UnsupportedCapability(_),
                        )) => (
                            CompactionExecutionMode::Local,
                            summarize_locally(provider, &request, compacted_prefix).await?,
                        ),
                        Err(error) => return Err(error),
                    }
                } else {
                    (
                        CompactionExecutionMode::Local,
                        summarize_locally(provider, &request, compacted_prefix).await?,
                    )
                }
            }
            CompactionMode::RemoteOnly => {
                if !supports_remote {
                    return Err(RuntimeError::FailedToCompactHistory(
                        ProviderError::UnsupportedCapability("history_compaction".to_string()),
                    ));
                }
                (
                    CompactionExecutionMode::Remote,
                    compact_remotely(provider, &request, compacted_prefix)
                        .await?
                        .ok_or_else(|| RuntimeError::FailedToCompactHistory(
                            ProviderError::UnsupportedCapability("history_compaction".to_string()),
                        ))?,
                )
            }
        };

        let preserved_user_turns = select_recent_user_turns(
            compacted_prefix,
            request.preserve_recent_user_tokens,
        );
        let preserved_delegation_results = select_recent_delegation_results(
            compacted_prefix,
            request.preserve_recent_delegation_results,
        );

        let mut replacement = Vec::new();
        replacement.extend(preserved_user_turns.iter().cloned());
        for item in &preserved_delegation_results {
            if !replacement.contains(item) {
                replacement.push(item.clone());
            }
        }
        replacement.push(TranscriptItem::compaction_summary(summary.clone()));
        replacement.extend_from_slice(&request.transcript.items()[preserve_from..]);

        Ok(Some(CompactionOutcome {
            mode,
            transcript_path,
            transcript: AgentTranscript::new(replacement),
            summary,
            replaced_items: compacted_prefix.len(),
            preserved_items: request.transcript.len().saturating_sub(preserve_from),
            preserved_user_turns: preserved_user_turns.len(),
            preserved_delegation_results: preserved_delegation_results.len(),
        }))
    }
}

pub(crate) fn compaction_request_from_agent(
    model: &str,
    transcript: AgentTranscript,
    config: &crate::agent::CompactionConfig,
    provider_request_options: ProviderRequestOptions,
) -> CompactionRequest {
    CompactionRequest {
        model: model.to_string(),
        transcript,
        transcript_dir: config.transcript_dir.clone(),
        summary_max_input_chars: config.summary_max_input_chars,
        summary_max_output_tokens: config.summary_max_output_tokens,
        preserve_recent_user_tokens: config.preserve_recent_user_tokens,
        preserve_recent_delegation_results: config.preserve_recent_delegation_results,
        provider_request_options,
        mode: config.mode,
    }
}

async fn summarize_locally(
    provider: Arc<dyn Provider>,
    request: &CompactionRequest,
    items: &[TranscriptItem],
) -> Result<CompactionSummary, RuntimeError> {
    let serialized = serde_json::to_string(items).map_err(RuntimeError::FailedToSerializeTranscript)?;
    let transcript = truncate_to_char_boundary(&serialized, request.summary_max_input_chars);
    let system = "You compress agent transcripts for continuity. Return strict JSON with keys: goal, progress, decisions, constraints, delegated_work, artifacts, open_questions, next_steps.";
    let prompt = format!(
        "Summarize this agent transcript for continuity and multi-agent handoff. Preserve goal, progress, concrete decisions, constraints, delegated work outcomes, artifacts, open questions, and next steps.\n\nTranscript JSON:\n{transcript}"
    );
    let response = provider
        .send(Request {
            model: Cow::Borrowed(request.model.as_str()),
            system: Some(Cow::Borrowed(system)),
            messages: Cow::Owned(vec![Message::user(ContentBlock::text(prompt))]),
            tools: Cow::Owned(Vec::new()),
            tool_choice: None,
            temperature: None,
            max_output_tokens: Some(request.summary_max_output_tokens),
            metadata: Cow::Owned(Default::default()),
            provider_request_options: request.provider_request_options.clone(),
        })
        .await
        .map_err(RuntimeError::FailedToCompactHistory)?;
    let text = response
        .content
        .into_iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    if text.is_empty() {
        return Ok(CompactionSummary::default());
    }

    serde_json::from_str(&text).unwrap_or_else(|_| CompactionSummary::from_fallback_text(text))
        .pipe(Ok)
}

async fn compact_remotely(
    provider: Arc<dyn Provider>,
    request: &CompactionRequest,
    items: &[TranscriptItem],
) -> Result<Option<CompactionSummary>, RuntimeError> {
    let input = items.iter().map(project_compaction_item).collect::<Vec<_>>();
    let response = provider
        .compact(ProviderCompactionRequest {
            model: Cow::Borrowed(request.model.as_str()),
            instructions: Cow::Borrowed(
                "Compact this transcript into a continuity handoff that preserves delegated work.",
            ),
            input: Cow::Owned(input),
            metadata: Cow::Owned(Default::default()),
            provider_request_options: request.provider_request_options.clone(),
        })
        .await
        .map_err(RuntimeError::FailedToCompactHistory)?;
    Ok(parse_remote_summary(response))
}

fn parse_remote_summary(response: ProviderCompactionResponse) -> Option<CompactionSummary> {
    response.output.into_iter().rev().find_map(|item| match item {
        CompactionInputItem::CompactionSummary { content } => {
            serde_json::from_str(&content)
                .ok()
                .or_else(|| Some(CompactionSummary::from_fallback_text(content)))
        }
        _ => None,
    })
}

fn project_compaction_item(item: &TranscriptItem) -> CompactionInputItem {
    match &item.kind {
        TranscriptKind::UserTurn => CompactionInputItem::UserTurn {
            content: item.text(),
        },
        TranscriptKind::AssistantTurn => CompactionInputItem::AssistantTurn {
            content: item.text(),
        },
        TranscriptKind::ToolExchange { is_error, .. } => CompactionInputItem::ToolExchange {
            request: None,
            result: item.text(),
            is_error: *is_error,
        },
        TranscriptKind::CanonicalContext => CompactionInputItem::CanonicalContext {
            content: item.text(),
        },
        TranscriptKind::MemoryRecall => CompactionInputItem::MemoryRecall {
            content: item.text(),
        },
        TranscriptKind::DelegationRequest { delegation, .. }
        | TranscriptKind::DelegationResult { delegation, .. } => CompactionInputItem::DelegationResult {
            agent_id: delegation.agent_id.clone(),
            agent_name: delegation.agent_name.clone(),
            role: delegation.role.clone(),
            status: format!("{:?}", delegation.status).to_lowercase(),
            content: item.text(),
        },
        TranscriptKind::CompactionSummary { summary } => CompactionInputItem::CompactionSummary {
            content: summary.render_for_handoff(),
        },
    }
}

fn select_recent_user_turns(items: &[TranscriptItem], token_budget: usize) -> Vec<TranscriptItem> {
    let mut selected = Vec::new();
    let mut remaining = token_budget;
    for item in items.iter().rev() {
        if !item.is_real_user_turn() {
            continue;
        }
        let tokens = approx_token_count(&item.text());
        if tokens > remaining && !selected.is_empty() {
            break;
        }
        remaining = remaining.saturating_sub(tokens);
        selected.push(item.clone());
        if remaining == 0 {
            break;
        }
    }
    selected.reverse();
    selected
}

fn select_recent_delegation_results(items: &[TranscriptItem], max_items: usize) -> Vec<TranscriptItem> {
    let mut selected = items
        .iter()
        .filter(|item| item.is_delegation_result())
        .rev()
        .take(max_items)
        .cloned()
        .collect::<Vec<_>>();
    selected.reverse();
    selected
}

fn required_tail_start_for_continuation(items: &[TranscriptItem]) -> usize {
    let Some(last_index) = items.len().checked_sub(1) else {
        return 0;
    };
    let last = &items[last_index];
    if matches!(last.kind, TranscriptKind::ToolExchange { .. })
        && last_index > 0
        && matches!(items[last_index - 1].kind, TranscriptKind::AssistantTurn)
    {
        last_index - 1
    } else {
        last_index
    }
}

fn approx_token_count(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

async fn persist_transcript(
    transcript: &[TranscriptItem],
    transcript_dir: &Path,
) -> Result<PathBuf, RuntimeError> {
    tokio::fs::create_dir_all(transcript_dir)
        .await
        .map_err(RuntimeError::FailedToPersistTranscript)?;

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let transcript_path = transcript_dir.join(format!("{timestamp}.jsonl"));
    let mut serialized = String::new();
    for item in transcript {
        let line =
            serde_json::to_string(item).map_err(RuntimeError::FailedToSerializeTranscript)?;
        serialized.push_str(&line);
        serialized.push('\n');
    }
    tokio::fs::write(&transcript_path, serialized)
        .await
        .map_err(RuntimeError::FailedToPersistTranscript)?;
    Ok(transcript_path)
}

fn truncate_to_char_boundary(input: &str, max_chars: usize) -> &str {
    if input.chars().count() <= max_chars {
        return input;
    }

    let mut end = input.len();
    for (index, _) in input.char_indices().take(max_chars + 1) {
        end = index;
    }
    &input[..end]
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}

impl<T> Pipe for T {}
