use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionContextBackendKind {
    GitWorktree,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionContextStatus {
    Active,
    Kept,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitWorktreeMetadata {
    pub branch: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionContextItem {
    pub name: String,
    pub backend: ExecutionContextBackendKind,
    pub path: PathBuf,
    #[serde(default)]
    pub task_id: Option<u64>,
    pub status: ExecutionContextStatus,
    #[serde(default)]
    pub git_worktree: Option<GitWorktreeMetadata>,
}
