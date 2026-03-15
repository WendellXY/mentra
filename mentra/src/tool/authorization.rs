use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::tool::{ToolCapability, ToolDurability, ToolSideEffectLevel};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolAuthorizationPreview {
    pub working_directory: PathBuf,
    pub capabilities: Vec<ToolCapability>,
    pub side_effect_level: ToolSideEffectLevel,
    pub durability: ToolDurability,
    pub raw_input: Value,
    pub structured_input: Value,
}
