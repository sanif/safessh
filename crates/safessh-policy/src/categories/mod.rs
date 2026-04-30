//! Command category matchers.
//!
//! For v0.1 Task 10 only the shell matchers are wired here. Task 11 will add
//! `pub mod sql;` plus a combining `match_all` that merges shell + SQL
//! categories into a sorted, deduped list.

pub mod shell;

pub use shell::match_shell_categories;
