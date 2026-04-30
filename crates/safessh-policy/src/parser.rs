//! Shell command parser used by the policy engine.
//!
//! # Safety invariant
//!
//! SAFETY-INVARIANT-1: on any parse failure, [`parse`] returns
//! [`ParseError::Opaque`] instead of a partial AST. Callers MUST treat
//! [`ParseError::Opaque`] as **default-deny** so that malformed or ambiguous
//! input never reaches a category matcher with a partially-populated
//! [`ParsedCommand`].
//!
//! # Strategy
//!
//! `v0.1` uses a hybrid approach:
//!
//! 1. Drive [`conch_parser::parse::DefaultParser`] over the raw input. If it
//!    yields an error on any iteration, return [`ParseError::Opaque`]. This
//!    catches genuine shell-syntax errors (unmatched backticks, bad
//!    redirection syntax, etc.).
//! 2. Tokenize the raw string with [`shell_words::split`], which respects
//!    quoting and reports unterminated quotes as an error (also mapped to
//!    [`ParseError::Opaque`]).
//! 3. Walk the resulting tokens and produce a single [`ParsedCommand`] per
//!    pipeline stage. The first token is the binary; subsequent tokens are
//!    classified as flags (when they start with `-`) or positional args.
//!    Bundled short flags (`-rf`) split into individual flags (`-r`, `-f`);
//!    long flags (`--all`) stay whole.
//!
//! Pipes, redirects, and env mutations are intentionally left empty for v0.1 —
//! the category matchers in Task 10 only need binary/flags/args. Multi-stage
//! pipelines and redirect handling land in a later task.

use crate::ast::ParsedCommand;
use thiserror::Error;

/// Errors returned by [`parse`].
///
/// The single `Opaque` variant is intentional: callers must default-deny on
/// any parse failure, so distinguishing failure modes would only encourage
/// callers to special-case (and likely under-deny) some of them.
#[derive(Debug, Error)]
pub enum ParseError {
    /// Parsing failed for some reason. The string is a human-readable
    /// description for logs/diagnostics; callers MUST NOT branch on its
    /// contents.
    #[error("opaque parse: {0}")]
    Opaque(String),
}

/// Parse `raw` into a sequence of [`ParsedCommand`]s, one per pipeline stage.
///
/// In v0.1 only the first pipeline stage is materialised: the returned vector
/// always has exactly one element on success.
///
/// Returns [`ParseError::Opaque`] for empty input, syntax errors detected by
/// `conch-parser`, or tokenisation failures (e.g. unterminated quotes).
pub fn parse(raw: &str) -> Result<Vec<ParsedCommand>, ParseError> {
    if raw.trim().is_empty() {
        return Err(ParseError::Opaque("empty command".into()));
    }

    // 1. Run conch-parser purely as a syntax validator. We do not walk its
    //    AST — its generic types are heavyweight and v0.1's category matchers
    //    only need the flat token view from shell-words.
    syntax_check(raw)?;

    // 2. Tokenise respecting POSIX quoting rules.
    let tokens =
        shell_words::split(raw).map_err(|e| ParseError::Opaque(format!("tokenize: {e}")))?;

    if tokens.is_empty() {
        return Err(ParseError::Opaque("no tokens".into()));
    }

    // 3. Build a single ParsedCommand from the tokens.
    let mut iter = tokens.into_iter();
    let binary = iter
        .next()
        .ok_or_else(|| ParseError::Opaque("no binary".into()))?;

    let mut flags: Vec<String> = Vec::new();
    let mut args: Vec<String> = Vec::new();
    for tok in iter {
        classify_token(&tok, &mut flags, &mut args);
    }

    Ok(vec![ParsedCommand {
        binary,
        flags,
        args,
        redirects: Vec::new(),
        pipes: Vec::new(),
        env_mutations: Vec::new(),
        raw: raw.to_string(),
    }])
}

/// Run conch-parser as a syntax-only validator. We discard the AST and only
/// care whether iteration yields an error.
///
/// Wrapped in `catch_unwind` because conch-parser 0.1.1 has been observed to
/// panic on some adversarial inputs; per SAFETY-INVARIANT-1 we must convert
/// any such panic into `ParseError::Opaque` rather than tearing down the
/// process.
fn syntax_check(raw: &str) -> Result<(), ParseError> {
    use std::panic::{catch_unwind, AssertUnwindSafe};

    let raw_owned = raw.to_string();
    let result = catch_unwind(AssertUnwindSafe(move || -> Result<(), String> {
        use conch_parser::lexer::Lexer;
        use conch_parser::parse::DefaultParser;

        let lex = Lexer::new(raw_owned.chars());
        let parser = DefaultParser::new(lex);
        for cmd in parser {
            cmd.map_err(|e| format!("conch: {e:?}"))?;
        }
        Ok(())
    }));

    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(msg)) => Err(ParseError::Opaque(msg)),
        Err(_panic) => Err(ParseError::Opaque("conch panic".into())),
    }
}

/// Classify a single token as a flag (one or more) or a positional arg.
///
/// * `--foo` — long flag, kept verbatim.
/// * `-rf` — bundled short flags, split into `-r`, `-f`.
/// * `-` alone — positional arg (stdin convention).
/// * anything else — positional arg.
fn classify_token(tok: &str, flags: &mut Vec<String>, args: &mut Vec<String>) {
    if tok.starts_with("--") {
        // Long flag. `--` alone is a separator but for v0.1 we treat it as a
        // flag-shaped token; matchers don't depend on this distinction.
        flags.push(tok.to_string());
    } else if tok.starts_with('-') && tok.len() > 1 {
        // Bundled short flags: split each character after the leading '-'.
        // Numeric short flags like `-1` still split (becomes `-1`), which is
        // correct for the typical `head -1` style.
        for c in tok[1..].chars() {
            flags.push(format!("-{c}"));
        }
    } else {
        // Bare `-` (stdin) or any other token is positional.
        args.push(tok.to_string());
    }
}
