/// Declares a stream agent whose only state is its read batch size. `new()` and
/// `Default` use `agent::DEFAULT_BATCH_SIZE`; unit tests can shrink it with
/// `with_batch_size` to exercise paging.
macro_rules! batch_sized_agent {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        pub struct $name {
            batch_size: usize,
        }

        impl $name {
            pub fn new() -> Self {
                Self {
                    batch_size: $crate::operations::streams::agent::DEFAULT_BATCH_SIZE,
                }
            }

            #[cfg(test)]
            pub fn with_batch_size(batch_size: usize) -> Self {
                Self { batch_size }
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

mod amp;
pub mod claude;
mod codex;
mod continue_cli;
mod copilot;
mod copilot_cli;
pub mod copilot_otel;
mod cursor;
mod discovery;
mod droid;
mod gemini;
pub mod opencode;
pub(crate) mod opencode_checkpoint;
mod pi;
#[cfg(test)]
pub(crate) mod test_support;
mod windsurf;

pub use amp::AmpAgent;
pub use claude::ClaudeAgent;
pub use codex::CodexAgent;
pub use continue_cli::ContinueAgent;
pub use copilot::CopilotAgent;
pub use copilot_cli::CopilotCliAgent;
pub use cursor::CursorAgent;
pub use droid::DroidAgent;
pub use gemini::GeminiAgent;
pub use opencode::OpenCodeAgent;
pub use pi::PiAgent;
pub use windsurf::WindsurfAgent;
