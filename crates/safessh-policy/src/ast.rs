//! Shell AST types for the policy engine.
//!
//! Re-exports `ParsedCommand` from `safessh-core` so all callers reference a
//! single canonical type.

pub use safessh_core::types::ParsedCommand;
