use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::Value;

use crate::runtime::BackgroundTaskSummary;
use crate::runtime::RuntimeHandle;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSpec {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
}

#[derive(Clone)]
pub struct ToolContext {
    pub agent_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub(crate) working_directory: PathBuf,
    pub(crate) runtime: RuntimeHandle,
}

impl ToolContext {
    pub fn working_directory(&self) -> &Path {
        self.working_directory.as_path()
    }

    pub fn resolve_working_directory(
        &self,
        working_directory: Option<&str>,
    ) -> Result<PathBuf, String> {
        self.runtime
            .resolve_working_directory(&self.agent_id, working_directory)
    }

    pub fn load_skill(&self, name: &str) -> Result<String, String> {
        self.runtime.load_skill(name)
    }

    pub fn skill_descriptions(&self) -> Option<String> {
        self.runtime.skill_descriptions()
    }

    pub fn start_background_task(&self, command: String, cwd: PathBuf) -> BackgroundTaskSummary {
        self.runtime
            .start_background_task(&self.agent_id, command, cwd)
    }

    pub fn check_background_task(&self, task_id: Option<&str>) -> Result<String, String> {
        self.runtime.check_background_task(&self.agent_id, task_id)
    }
}

pub type ToolResult = Result<String, String>;

#[async_trait]
pub trait ToolHandler: Send + Sync {
    fn spec(&self) -> ToolSpec;

    async fn invoke(&self, ctx: ToolContext, input: Value) -> ToolResult;
}
