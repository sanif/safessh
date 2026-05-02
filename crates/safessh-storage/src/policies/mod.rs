//! Binary-embedded preset policy rules.
//!
//! Parsed once at first access. Returned as a `&'static [FileRule]`.

use crate::project::FileRule;
use serde::Deserialize;
use std::sync::OnceLock;

static PRESETS_TOML: &str = include_str!("presets.toml");

#[derive(Deserialize)]
struct PresetFile {
    #[serde(default)]
    file_rules: Vec<FileRule>,
}

static PARSED: OnceLock<Vec<FileRule>> = OnceLock::new();

/// Returns the binary-embedded deny-list of sensitive paths. Parsed once;
/// subsequent calls return the cached slice.
pub fn preset_file_rules() -> &'static [FileRule] {
    PARSED
        .get_or_init(|| {
            toml::from_str::<PresetFile>(PRESETS_TOML)
                .expect("BUG: presets.toml is malformed")
                .file_rules
        })
        .as_slice()
}
