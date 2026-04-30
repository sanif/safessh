//! Shared core types used across the safessh workspace.

use crate::error::{Error, Result};
use chrono::{DateTime, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Newtype wrapper around a project identifier with validation.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct ProjectId(String);

impl ProjectId {
    /// Construct a `ProjectId`, rejecting empty strings, slashes, spaces,
    /// backslashes, and leading dots.
    pub fn new(s: impl Into<String>) -> Result<Self> {
        let s = s.into();
        if s.is_empty()
            || s.contains('/')
            || s.contains(' ')
            || s.contains('\\')
            || s.starts_with('.')
        {
            return Err(Error::Usage(format!("invalid project id: {s:?}")));
        }
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A connection target: either an SSH config alias or an inline definition.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Target {
    SshConfigAlias {
        name: String,
        ssh_config_alias: String,
    },
    Inline {
        name: String,
        host: String,
        #[serde(default = "default_port")]
        port: u16,
        user: String,
        identity_file: Option<PathBuf>,
        proxy_jump: Option<String>,
        keychain_secret: Option<String>,
    },
}

fn default_port() -> u16 {
    22
}

impl Target {
    pub fn name(&self) -> &str {
        match self {
            Target::SshConfigAlias { name, .. } | Target::Inline { name, .. } => name,
        }
    }
}

/// A parsed shell command, with recursive `pipes` for piped subcommands.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParsedCommand {
    pub binary: String,
    pub flags: Vec<String>,
    pub args: Vec<String>,
    pub redirects: Vec<String>,
    pub pipes: Vec<ParsedCommand>,
    pub env_mutations: Vec<(String, String)>,
    pub raw: String,
}

/// The outcome of policy evaluation for a single command.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PolicyDecision {
    Allow {
        matched_rule: Option<String>,
        source: AllowSource,
    },
    RequireApproval {
        token: ApprovalToken,
        categories: Vec<String>,
        reason: String,
    },
    Block {
        rule_id: String,
        pattern: String,
    },
    Deny {
        reason: String,
    },
}

/// Why a command was allowed.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AllowSource {
    DefaultPolicy,
    AlwaysRule(String),
    TimedRule {
        rule_id: String,
        expires_at: DateTime<Utc>,
    },
    Yolo,
}

/// A 6-character alphanumeric token used to identify a pending approval.
///
/// The alphabet excludes 0, 1, l, and o for readability.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ApprovalToken(String);

impl ApprovalToken {
    pub fn generate() -> Self {
        // No 0/1/l/o for readability.
        const ALPHABET: &[u8] = b"abcdefghijkmnpqrstuvwxyz23456789";
        let mut rng = rand::thread_rng();
        let token: String = (0..6)
            .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
            .collect();
        Self(token)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A single audit log entry.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuditEvent {
    pub schema_version: u32,
    pub timestamp: DateTime<Utc>,
    pub event_type: String,
    pub project: Option<String>,
    pub data: serde_json::Value,
    pub error_class: Option<String>,
    pub error_message: Option<String>,
}

impl AuditEvent {
    pub fn new(event_type: impl Into<String>) -> Self {
        Self {
            schema_version: 1,
            timestamp: Utc::now(),
            event_type: event_type.into(),
            project: None,
            data: serde_json::Value::Null,
            error_class: None,
            error_message: None,
        }
    }
}
