mod actor;
mod intrinsic;
mod manager;
mod prompt;
mod store;
mod types;

pub(crate) use actor::teammate_actor_loop;
pub(crate) use manager::TeamManager;
pub(crate) use prompt::{TEAMMATE_MAX_ROUNDS, build_teammate_system_prompt};
pub(crate) use types::format_inbox;
pub use types::{
    TeamDispatch, TeamMemberStatus, TeamMemberSummary, TeamMessage, TeamProtocolRequestSummary,
    TeamProtocolStatus,
};
pub(crate) use types::{TeamRequestDirection, TeamRequestFilter};

use crate::{ContentBlock, runtime::Agent, tool::ToolCall};

pub(crate) use intrinsic::intrinsic_specs;

pub(crate) const TEAM_SPAWN_TOOL_NAME: &str = "team_spawn";
pub(crate) const TEAM_SEND_TOOL_NAME: &str = "team_send";
pub(crate) const TEAM_READ_INBOX_TOOL_NAME: &str = "team_read_inbox";
pub(crate) const TEAM_BROADCAST_TOOL_NAME: &str = "broadcast";
pub(crate) const TEAM_REQUEST_TOOL_NAME: &str = "team_request";
pub(crate) const TEAM_RESPOND_TOOL_NAME: &str = "team_respond";
pub(crate) const TEAM_LIST_REQUESTS_TOOL_NAME: &str = "team_list_requests";

pub(crate) fn is_team_tool(name: &str) -> bool {
    matches!(
        name,
        TEAM_SPAWN_TOOL_NAME
            | TEAM_SEND_TOOL_NAME
            | TEAM_READ_INBOX_TOOL_NAME
            | TEAM_BROADCAST_TOOL_NAME
            | TEAM_REQUEST_TOOL_NAME
            | TEAM_RESPOND_TOOL_NAME
            | TEAM_LIST_REQUESTS_TOOL_NAME
    )
}

pub(crate) async fn execute_intrinsic(agent: &mut Agent, call: ToolCall) -> Option<ContentBlock> {
    let result = match call.name.as_str() {
        TEAM_SPAWN_TOOL_NAME => intrinsic::execute_team_spawn(agent, call).await,
        TEAM_SEND_TOOL_NAME => intrinsic::execute_team_send(agent, call),
        TEAM_READ_INBOX_TOOL_NAME => intrinsic::execute_team_read_inbox(agent, call),
        TEAM_BROADCAST_TOOL_NAME => intrinsic::execute_team_broadcast(agent, call),
        TEAM_REQUEST_TOOL_NAME => intrinsic::execute_team_request(agent, call),
        TEAM_RESPOND_TOOL_NAME => intrinsic::execute_team_respond(agent, call),
        TEAM_LIST_REQUESTS_TOOL_NAME => intrinsic::execute_team_list_requests(agent, call),
        _ => return None,
    };
    Some(result)
}
