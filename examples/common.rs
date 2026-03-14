use dotenvy::dotenv;
use mentra::{BuiltinProvider, ModelInfo, ModelSelector, Runtime, RuntimePolicy};

#[allow(dead_code)]
pub fn openai_runtime() -> Result<Runtime, Box<dyn std::error::Error>> {
    dotenv().ok();

    let api_key = std::env::var("OPENAI_API_KEY")
        .map_err(|_| "OPENAI_API_KEY must be set before running this example")?;

    Ok(Runtime::builder()
        .with_provider(BuiltinProvider::OpenAI, api_key)
        .with_policy(RuntimePolicy::permissive())
        .build()?)
}

pub async fn openai_model(runtime: &Runtime) -> Result<ModelInfo, Box<dyn std::error::Error>> {
    let selector = std::env::var("MENTRA_MODEL")
        .map(ModelSelector::Id)
        .unwrap_or(ModelSelector::NewestAvailable);

    Ok(runtime
        .resolve_model(BuiltinProvider::OpenAI, selector)
        .await?)
}

#[allow(dead_code)]
pub fn first_arg_or(default: &str) -> String {
    std::env::args()
        .nth(1)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
}
