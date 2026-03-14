mod common;

use std::time::Duration;

use mentra::agent::{AgentConfig, TeamAutonomyConfig, TeamConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = common::openai_runtime()?;
    let model = common::openai_model(&runtime).await?;

    let mut lead = runtime.spawn_with_config(
        "Lead",
        model,
        AgentConfig {
            team: TeamConfig {
                autonomy: TeamAutonomyConfig {
                    enabled: true,
                    poll_interval: Duration::from_secs(1),
                    idle_timeout: Duration::from_secs(15),
                },
                ..Default::default()
            },
            ..Default::default()
        },
    )?;

    let teammate = lead
        .spawn_teammate(
            "Researcher",
            "Summarizes findings for the lead",
            Some("Introduce yourself to the lead in one concise sentence.".to_string()),
        )
        .await?;

    println!("Spawned teammate {} ({})", teammate.name, teammate.id);

    let request = common::first_arg_or("Reply with one sentence on why tool-using agents matter.");
    lead.send_team_message("Researcher", request)?;

    tokio::time::sleep(Duration::from_secs(2)).await;

    let inbox = lead.read_team_inbox()?;
    if inbox.is_empty() {
        println!("No teammate replies were available yet.");
    } else {
        for message in inbox {
            println!("[{}] {}: {}", message.kind, message.sender, message.content);
        }
    }

    Ok(())
}
