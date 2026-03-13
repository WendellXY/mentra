use std::io::{self, Read, Write};

use dotenvy::dotenv;
use mentra::{
    Agent, BuiltinProvider, ContentBlock, ModelInfo, ProviderId, Runtime, agent::AgentEvent,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();

    let prompt = read_prompt()?;
    let api_key = std::env::var("OPENAI_API_KEY")
        .map_err(|_| "OPENAI_API_KEY must be set before running the quickstart example")?;

    let runtime = Runtime::builder()
        .with_provider(BuiltinProvider::OpenAI, api_key)
        .build()?;

    let model = pick_model(&runtime).await?;
    let mut agent = runtime.spawn("Quickstart", model)?;
    stream_response(&mut agent, prompt).await?;
    Ok(())
}

fn read_prompt() -> Result<String, Box<dyn std::error::Error>> {
    let from_args = std::env::args().skip(1).collect::<Vec<_>>();
    if !from_args.is_empty() {
        return Ok(from_args.join(" "));
    }

    eprint!("Enter a prompt: ");
    io::stderr().flush()?;

    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let prompt = input.trim().to_string();
    if prompt.is_empty() {
        return Err("Provide a prompt as CLI args or via stdin".into());
    }

    Ok(prompt)
}

async fn pick_model(runtime: &Runtime) -> Result<ModelInfo, Box<dyn std::error::Error>> {
    if let Ok(model) = std::env::var("MENTRA_MODEL") {
        return Ok(ModelInfo {
            id: model,
            provider: ProviderId::from(BuiltinProvider::OpenAI),
            display_name: None,
            description: None,
            created_at: None,
        });
    }

    let provider_id = ProviderId::from(BuiltinProvider::OpenAI);
    let mut models = runtime.list_models(Some(&provider_id)).await?;
    models.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });

    models
        .into_iter()
        .next()
        .ok_or_else(|| "OpenAI did not return any models".into())
}

async fn stream_response(
    agent: &mut Agent,
    prompt: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut events = agent.subscribe_events();
    let printer = tokio::spawn(async move {
        while let Ok(event) = events.recv().await {
            match event {
                AgentEvent::TextDelta { delta, .. } => {
                    print!("{delta}");
                    let _ = io::stdout().flush();
                }
                AgentEvent::RunFinished => {
                    println!();
                    break;
                }
                AgentEvent::RunFailed { error } => {
                    eprintln!("\nRun failed: {error}");
                    break;
                }
                _ => {}
            }
        }
    });

    agent.send(vec![ContentBlock::text(prompt)]).await?;
    printer.await?;
    Ok(())
}
