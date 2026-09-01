//! Forgiving CLI Parser
//!
//! Provides typo-tolerant command parsing for AI agents and human users.
//! Uses edit distance algorithms to auto-correct common typos and suggest
//! alternatives for unknown commands.

// Cross-binary test API: consumed by `mod tests` and/or sibling `tests/*.rs` files; the bin build does not call it.
#[allow(dead_code)]
pub mod aliases;
// Cross-binary test API: consumed by `mod tests` and/or sibling `tests/*.rs` files; the bin build does not call it.
#[allow(dead_code)]
pub mod parser;
// Cross-binary test API: consumed by `mod tests` and/or sibling `tests/*.rs` files; the bin build does not call it.
#[allow(dead_code)]
pub mod suggestions;

#[allow(unused_imports)]
pub use aliases::{AliasRegistry, DEFAULT_ALIASES};
#[allow(unused_imports)]
pub use parser::{ForgivingParser, ParseResult};
#[allow(unused_imports)]
pub use suggestions::CommandSuggestion;
