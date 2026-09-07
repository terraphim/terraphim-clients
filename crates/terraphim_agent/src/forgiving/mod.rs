//! Forgiving CLI Parser
//!
//! Provides typo-tolerant command parsing for AI agents and human users.
//! Uses edit distance algorithms to auto-correct common typos and suggest
//! alternatives for unknown commands.

pub mod aliases;
pub mod parser;
pub mod suggestions;

#[allow(unused_imports)]
pub use aliases::{AliasRegistry, DEFAULT_ALIASES};
#[allow(unused_imports)]
pub use parser::{ForgivingParser, ParseResult};
#[allow(unused_imports)]
pub use suggestions::CommandSuggestion;
