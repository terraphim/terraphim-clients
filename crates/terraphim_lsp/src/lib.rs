//! Language Server Protocol (LSP) support for Terraphim knowledge graphs.
//!
//! Provides LSP diagnostics for KG markdown and Rust files via the
//! Explicit Deferral Marker (EDM) scanner, and Aho-Corasick KG term matching
//! for hover/completion support, enabling editor support for authoring
//! Terraphim knowledge-graph content.

mod config;
mod diagnostic;
pub mod kg_analysis;
mod server;

pub use config::LspConfig;
pub use diagnostic::finding_to_diagnostic;
pub use kg_analysis::{KgAnalysis, TermMatch, analyse_kg_document};
pub use server::TerraphimLspServer;
