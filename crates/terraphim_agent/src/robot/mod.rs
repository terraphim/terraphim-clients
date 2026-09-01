//! Robot Mode - Machine-readable output for AI agents
//!
//! This module provides structured JSON output and self-documentation
//! capabilities for integration with AI agents and automation tools.

// Cross-binary test API: consumed by `mod tests` and/or sibling `tests/*.rs` files; the bin build does not call it.
#[allow(dead_code)]
pub mod budget;
// Cross-binary test API: consumed by `mod tests` and/or sibling `tests/*.rs` files; the bin build does not call it.
#[allow(dead_code)]
pub mod docs;
// Cross-binary test API: consumed by `mod tests` and/or sibling `tests/*.rs` files; the bin build does not call it.
#[allow(dead_code)]
pub mod exit_codes;
// Cross-binary test API: consumed by `mod tests` and/or sibling `tests/*.rs` files; the bin build does not call it.
#[allow(dead_code)]
pub mod output;
// Cross-binary test API: consumed by `mod tests` and/or sibling `tests/*.rs` files; the bin build does not call it.
#[allow(dead_code)]
pub mod schema;

#[allow(unused_imports)]
pub use budget::{BudgetEngine, BudgetError, BudgetedResults};
#[allow(unused_imports)]
pub use docs::{ArgumentDoc, Capabilities, CommandDoc, ExampleDoc, FlagDoc, SelfDocumentation};
#[allow(unused_imports)]
pub use exit_codes::ExitCode;
#[allow(unused_imports)]
pub use output::{FieldMode, OutputFormat, RobotConfig, RobotFormatter};
#[allow(unused_imports)]
pub use schema::{
    AutoCorrection, Pagination, ResponseMeta, RobotError, RobotResponse, TokenBudget,
};
