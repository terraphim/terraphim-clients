//! Robot Mode - Machine-readable output for AI agents
//!
//! This module provides structured JSON output and self-documentation
//! capabilities for integration with AI agents and automation tools.

pub mod budget;
pub mod docs;
pub mod exit_codes;
pub mod output;
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
