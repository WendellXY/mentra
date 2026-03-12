mod execute;
mod schema;

use async_trait::async_trait;

use crate::{
    ContentBlock,
    tool::{ExecutableTool, ToolCall, ToolContext, ToolResult, ToolSpec},
};

pub(crate) use schema::intrinsic_specs;

#[derive(Clone, Copy)]
pub(crate) enum TeamIntrinsicTool {
    Spawn,
    Send,
    ReadInbox,
    Broadcast,
    Request,
    Respond,
    ListRequests,
}

impl TeamIntrinsicTool {
    fn all() -> [Self; 7] {
        [
            Self::Spawn,
            Self::Send,
            Self::ReadInbox,
            Self::Broadcast,
            Self::Request,
            Self::Respond,
            Self::ListRequests,
        ]
    }

    fn spec(self) -> ToolSpec {
        schema::tool_spec(self)
    }
}

#[async_trait]
impl ExecutableTool for TeamIntrinsicTool {
    fn spec(&self) -> ToolSpec {
        (*self).spec()
    }

    async fn execute(&self, ctx: ToolContext<'_>, input: serde_json::Value) -> ToolResult {
        let call = ToolCall {
            id: ctx.tool_call_id.clone(),
            name: self.spec().name,
            input,
        };
        let block = match self {
            Self::Spawn => execute::execute_team_spawn(ctx.agent, call).await,
            Self::Send => execute::execute_team_send(ctx.agent, call),
            Self::ReadInbox => execute::execute_team_read_inbox(ctx.agent, call),
            Self::Broadcast => execute::execute_team_broadcast(ctx.agent, call),
            Self::Request => execute::execute_team_request(ctx.agent, call),
            Self::Respond => execute::execute_team_respond(ctx.agent, call),
            Self::ListRequests => execute::execute_team_list_requests(ctx.agent, call),
        };
        content_block_to_result(block)
    }
}

pub(crate) fn register_tools(registry: &mut crate::tool::ToolRegistry) {
    for tool in TeamIntrinsicTool::all() {
        registry.register_tool(tool);
    }
}

fn content_block_to_result(block: ContentBlock) -> ToolResult {
    match block {
        ContentBlock::ToolResult {
            content, is_error, ..
        } => {
            if is_error {
                Err(content)
            } else {
                Ok(content)
            }
        }
        _ => Err("Team intrinsic returned an unexpected content block".to_string()),
    }
}
