use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use tokio::sync::{Mutex as AsyncMutex, mpsc};

use crate::{Agent, ContentBlock, agent::TeamAutonomyConfig, error::RuntimeError};

use super::{TeamManager, TeamMemberStatus};

const TEAM_WAKE_PROMPT: &str = "Process any new team inbox messages and continue your work.";
const BACKGROUND_WAKE_PROMPT: &str =
    "Review any completed background task results and continue your work.";
const BACKGROUND_IDLE_POLL_INTERVAL: Duration = Duration::from_millis(25);

pub(crate) async fn teammate_actor_loop(
    manager: TeamManager,
    team_dir: PathBuf,
    teammate_name: String,
    agent: Arc<AsyncMutex<Agent>>,
    mut wake_rx: mpsc::UnboundedReceiver<()>,
) {
    while wait_for_wake_or_background(&agent, &mut wake_rx).await {
        match work_cycle(&manager, &team_dir, &teammate_name, &agent).await {
            Ok(true) | Err(()) => {}
            Ok(false) => break,
        }
    }

    let _ = manager.unregister_teammate_actor(&team_dir, &teammate_name);
}

async fn wait_for_wake_or_background(
    agent: &Arc<AsyncMutex<Agent>>,
    wake_rx: &mut mpsc::UnboundedReceiver<()>,
) -> bool {
    loop {
        let has_background_notifications = {
            let guard = agent.lock().await;
            guard
                .runtime_handle()
                .has_pending_background_notifications(guard.id())
        };
        if has_background_notifications {
            return true;
        }

        match tokio::time::timeout(BACKGROUND_IDLE_POLL_INTERVAL, wake_rx.recv()).await {
            Ok(Some(())) => return true,
            Ok(None) => return false,
            Err(_) => continue,
        }
    }
}

async fn work_cycle(
    manager: &TeamManager,
    team_dir: &Path,
    teammate_name: &str,
    agent: &Arc<AsyncMutex<Agent>>,
) -> Result<bool, ()> {
    let autonomy = {
        let guard = agent.lock().await;
        guard.config().team.autonomy.clone()
    };
    let mut next_prompt = None;

    loop {
        let prompt = match next_prompt.take() {
            Some(prompt) => prompt,
            None => match manager.has_pending_messages(team_dir, teammate_name) {
                Ok(true) => TEAM_WAKE_PROMPT.to_string(),
                Ok(false) => {
                    let has_background_notifications = {
                        let guard = agent.lock().await;
                        guard
                            .runtime_handle()
                            .has_pending_background_notifications(guard.id())
                    };
                    if has_background_notifications {
                        BACKGROUND_WAKE_PROMPT.to_string()
                    } else {
                        match manager.take_shutdown_signal(team_dir, teammate_name) {
                            Ok(true) => {
                                let _ = manager.update_member_status(
                                    team_dir,
                                    teammate_name,
                                    TeamMemberStatus::Shutdown,
                                );
                                return Ok(false);
                            }
                            Ok(false) => {}
                            Err(error) => {
                                let _ = mark_failed(manager, team_dir, teammate_name, error);
                                return Err(());
                            }
                        }
                        if autonomy.enabled {
                            match idle_poll(manager, team_dir, teammate_name, agent, &autonomy)
                                .await
                            {
                                Ok(Some(prompt)) => prompt,
                                Ok(None) => {
                                    let _ = manager.update_member_status(
                                        team_dir,
                                        teammate_name,
                                        TeamMemberStatus::Shutdown,
                                    );
                                    return Ok(false);
                                }
                                Err(error) => {
                                    let _ = mark_failed(manager, team_dir, teammate_name, error);
                                    return Err(());
                                }
                            }
                        } else {
                            let _ = manager.update_member_status(
                                team_dir,
                                teammate_name,
                                TeamMemberStatus::Idle,
                            );
                            return Ok(true);
                        }
                    }
                }
                Err(error) => {
                    let _ = mark_failed(manager, team_dir, teammate_name, error);
                    return Err(());
                }
            },
        };

        let _ = manager.update_member_status(team_dir, teammate_name, TeamMemberStatus::Working);
        let result = {
            let mut guard = agent.lock().await;
            guard.send(vec![ContentBlock::Text { text: prompt }]).await
        };

        match result {
            Ok(_) | Err(RuntimeError::EmptyAssistantResponse) => {
                next_prompt = None;
            }
            Err(error) => {
                let _ = mark_failed(manager, team_dir, teammate_name, error);
                return Err(());
            }
        }
    }
}

async fn idle_poll(
    manager: &TeamManager,
    team_dir: &Path,
    teammate_name: &str,
    agent: &Arc<AsyncMutex<Agent>>,
    autonomy: &TeamAutonomyConfig,
) -> Result<Option<String>, RuntimeError> {
    manager.update_member_status(team_dir, teammate_name, TeamMemberStatus::Idle)?;
    let idle_ms = autonomy.idle_timeout.as_millis();
    let poll_ms = autonomy.poll_interval.as_millis().max(1);
    let poll_count = idle_ms.div_ceil(poll_ms);

    for _ in 0..poll_count {
        tokio::time::sleep(autonomy.poll_interval).await;
        if manager.take_shutdown_signal(team_dir, teammate_name)? {
            return Ok(None);
        }
        if manager.has_pending_messages(team_dir, teammate_name)? {
            return Ok(Some(TEAM_WAKE_PROMPT.to_string()));
        }
        let has_background_notifications = {
            let guard = agent.lock().await;
            guard
                .runtime_handle()
                .has_pending_background_notifications(guard.id())
        };
        if has_background_notifications {
            return Ok(Some(BACKGROUND_WAKE_PROMPT.to_string()));
        }

        let claimed = {
            let mut guard = agent.lock().await;
            guard.try_claim_ready_task()?
        };
        if let Some(task) = claimed {
            let task_body = if task.description.trim().is_empty() {
                format!("Task #{}: {}", task.id, task.subject)
            } else {
                format!(
                    "Task #{}: {}\nDescription: {}",
                    task.id, task.subject, task.description
                )
            };
            return Ok(Some(format!(
                "<auto-claimed>{task_body}</auto-claimed>\n<reminder>Update your task status. Mark it in_progress when you start and completed when you finish.</reminder>"
            )));
        }
    }

    Ok(None)
}

fn mark_failed(
    manager: &TeamManager,
    team_dir: &Path,
    teammate_name: &str,
    error: RuntimeError,
) -> Result<(), RuntimeError> {
    manager.update_member_status(
        team_dir,
        teammate_name,
        TeamMemberStatus::Failed(error.to_string()),
    )
}
