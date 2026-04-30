//! Inline approval prompt for the TTY path.
//!
//! When stdin is a TTY, the `RequireApproval` arm of `safessh exec` calls
//! [`ask`] to present a five-action `dialoguer::Select` (`once`, `timed`,
//! `always`, `deny`, `block`). The headless path (no TTY) keeps emitting the
//! structured `BLOCKED:` token introduced in Task 20 and is unaffected by
//! this module.
//!
//! Tests inject deterministic answers via the `SAFESSH_PROMPT_RESPONSE`
//! environment variable. Recognised values are exactly `once`, `timed`,
//! `timed:N`, `always`, `deny`, and `block`; unknown values fall back to
//! `Deny` so a malformed override never silently approves a command.

use safessh_core::error::{Error, Result};
use safessh_core::types::ParsedCommand;

/// The user's selection at the inline approval prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptAction {
    /// Run this invocation only; no persistent rule.
    Once,
    /// Add a `TimedRule` that expires in `N` minutes, then run.
    Timed(u32),
    /// Add a permanent `PatternRule` to the project's `always` store, then run.
    Always,
    /// Deny this invocation; no persistent rule.
    Deny,
    /// Add a permanent `PatternRule` to the project's `blocked` store and deny.
    Block,
}

/// Prompt the user for an approval action.
///
/// Honours `SAFESSH_PROMPT_RESPONSE` for deterministic test injection; in
/// non-test runs presents an interactive `dialoguer::Select`.
pub fn ask(
    parsed: &ParsedCommand,
    categories: &[String],
    default_minutes: u32,
) -> Result<PromptAction> {
    if let Ok(canned) = std::env::var("SAFESSH_PROMPT_RESPONSE") {
        return Ok(parse_canned(&canned, default_minutes));
    }

    use dialoguer::Select;
    eprintln!("Command: {}", parsed.raw);
    eprintln!("Categories: {}", categories.join(", "));
    let items = &["once", "timed", "always", "deny", "block"];
    let sel = Select::new()
        .with_prompt("Action")
        .items(items)
        .default(0)
        .interact()
        .map_err(|e| Error::Usage(e.to_string()))?;
    Ok(match sel {
        0 => PromptAction::Once,
        1 => PromptAction::Timed(default_minutes),
        2 => PromptAction::Always,
        3 => PromptAction::Deny,
        _ => PromptAction::Block,
    })
}

/// Parse a `SAFESSH_PROMPT_RESPONSE` value. Unknown strings collapse to
/// `Deny` so a typo cannot accidentally grant approval.
fn parse_canned(s: &str, default_minutes: u32) -> PromptAction {
    if let Some(rest) = s.strip_prefix("timed:") {
        let m = rest.parse().unwrap_or(default_minutes);
        return PromptAction::Timed(m);
    }
    match s {
        "once" => PromptAction::Once,
        "timed" => PromptAction::Timed(default_minutes),
        "always" => PromptAction::Always,
        "deny" => PromptAction::Deny,
        "block" => PromptAction::Block,
        _ => PromptAction::Deny,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_parsed() -> ParsedCommand {
        ParsedCommand {
            binary: "rm".into(),
            flags: vec!["-r".into()],
            args: vec!["/tmp".into()],
            redirects: vec![],
            pipes: vec![],
            env_mutations: vec![],
            raw: "rm -r /tmp".into(),
        }
    }

    /// Tests in this file mutate the process environment via
    /// `SAFESSH_PROMPT_RESPONSE`. Cargo runs tests in a single binary on
    /// multiple threads by default, so we serialize to keep the env var
    /// predictable per assertion.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn canned_response_once() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("SAFESSH_PROMPT_RESPONSE", "once");
        let r = ask(&sample_parsed(), &["destructive:filesystem".into()], 30).unwrap();
        std::env::remove_var("SAFESSH_PROMPT_RESPONSE");
        assert!(matches!(r, PromptAction::Once));
    }

    #[test]
    fn canned_response_timed_with_minutes() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("SAFESSH_PROMPT_RESPONSE", "timed:15");
        let r = ask(&sample_parsed(), &["destructive:filesystem".into()], 30).unwrap();
        std::env::remove_var("SAFESSH_PROMPT_RESPONSE");
        assert!(matches!(r, PromptAction::Timed(15)));
    }

    #[test]
    fn canned_response_timed_bare_uses_default() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("SAFESSH_PROMPT_RESPONSE", "timed");
        let r = ask(&sample_parsed(), &[], 30).unwrap();
        std::env::remove_var("SAFESSH_PROMPT_RESPONSE");
        assert!(matches!(r, PromptAction::Timed(30)));
    }

    #[test]
    fn canned_response_always() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("SAFESSH_PROMPT_RESPONSE", "always");
        let r = ask(&sample_parsed(), &[], 30).unwrap();
        std::env::remove_var("SAFESSH_PROMPT_RESPONSE");
        assert!(matches!(r, PromptAction::Always));
    }

    #[test]
    fn canned_response_deny() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("SAFESSH_PROMPT_RESPONSE", "deny");
        let r = ask(&sample_parsed(), &[], 30).unwrap();
        std::env::remove_var("SAFESSH_PROMPT_RESPONSE");
        assert!(matches!(r, PromptAction::Deny));
    }

    #[test]
    fn canned_response_block() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("SAFESSH_PROMPT_RESPONSE", "block");
        let r = ask(&sample_parsed(), &[], 30).unwrap();
        std::env::remove_var("SAFESSH_PROMPT_RESPONSE");
        assert!(matches!(r, PromptAction::Block));
    }

    #[test]
    fn canned_response_unknown_falls_back_to_deny() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("SAFESSH_PROMPT_RESPONSE", "garbage");
        let r = ask(&sample_parsed(), &[], 30).unwrap();
        std::env::remove_var("SAFESSH_PROMPT_RESPONSE");
        assert!(matches!(r, PromptAction::Deny));
    }
}
