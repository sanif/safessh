//! safessh-policy — shell/SQL parsers, command categorization, decision engine.

pub mod ast;
pub mod categories;
pub mod decision;
pub mod parser;

pub use decision::{decide, DecisionInput, FileOp, TunnelOp};
