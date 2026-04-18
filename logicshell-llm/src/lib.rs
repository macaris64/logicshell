// logicshell-llm: LLM bridge, context, composer, Ollama client
// Enable HTTP backend with the `ollama` feature flag.

pub mod client;
pub mod context;
pub mod error;
pub mod prompt;

pub use client::{LlmClient, LlmRequest, LlmResponse};
pub use context::{SystemContextProvider, SystemContextSnapshot};
pub use error::LlmError;
pub use prompt::PromptComposer;
