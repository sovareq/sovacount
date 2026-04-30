//! Compile-time-embedded classifier prompt.
//!
//! Users may override at runtime via `GOVERNOR_CLASSIFIER_PROMPT_FILE` or by
//! placing a custom prompt at `~/.config/token-governor/classifier-prompt.md`.

/// The default classifier system-prompt, embedded into the binary at compile time.
pub const DEFAULT_CLASSIFIER_PROMPT: &str = include_str!("prompts/classifier.md");
