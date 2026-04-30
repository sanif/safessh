//! Output redactor — strips secrets from bytes before they leave safessh's process.

use regex::bytes::Regex;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RedactionType {
    AwsAccessKey,
    AwsSecretKey,
    BearerToken,
    JwtToken,
    PrivateKeyBlock,
    PasswordParam,
    Custom(&'static str),
}

impl RedactionType {
    fn placeholder(&self) -> &'static [u8] {
        match self {
            RedactionType::AwsAccessKey => b"<REDACTED:aws_access_key>",
            RedactionType::AwsSecretKey => b"<REDACTED:aws_secret_key>",
            RedactionType::BearerToken => b"<REDACTED:bearer_token>",
            RedactionType::JwtToken => b"<REDACTED:jwt>",
            RedactionType::PrivateKeyBlock => b"<REDACTED:private_key_block>",
            RedactionType::PasswordParam => b"<REDACTED:password>",
            RedactionType::Custom(_) => b"<REDACTED:custom>",
        }
    }
}

pub struct Redactor {
    patterns: Vec<(RedactionType, Regex)>,
}

impl Default for Redactor {
    fn default() -> Self {
        let patterns = vec![
            (
                RedactionType::AwsAccessKey,
                Regex::new(r"AKIA[0-9A-Z]{16}").unwrap(),
            ),
            (
                RedactionType::AwsSecretKey,
                Regex::new(r"(?i)aws_secret_access_key\s*[=:]\s*[A-Za-z0-9/+=]{40}").unwrap(),
            ),
            (
                RedactionType::BearerToken,
                Regex::new(r"(?i)Bearer\s+[A-Za-z0-9._\-+/=]+").unwrap(),
            ),
            (
                RedactionType::JwtToken,
                Regex::new(r"eyJ[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+").unwrap(),
            ),
            (
                RedactionType::PrivateKeyBlock,
                Regex::new(
                    r"(?s)-----BEGIN [A-Z ]+PRIVATE KEY-----.+?-----END [A-Z ]+PRIVATE KEY-----",
                )
                .unwrap(),
            ),
            (
                RedactionType::PasswordParam,
                Regex::new(r"(?i)password=[^\s&]+").unwrap(),
            ),
        ];
        Self { patterns }
    }
}

impl Redactor {
    pub fn with_pattern(mut self, ty: RedactionType, regex: Regex) -> Self {
        self.patterns.push((ty, regex));
        self
    }

    // SAFETY-INVARIANT-6: This is the last byte transformation before output leaves
    // safessh's process. Any caller that emits bytes to the LLM/user must pass them
    // through `redact()` first.
    pub fn redact(&self, input: &[u8]) -> (Vec<u8>, HashMap<RedactionType, usize>) {
        let mut current = input.to_vec();
        let mut counts: HashMap<RedactionType, usize> = HashMap::new();
        for (ty, re) in &self.patterns {
            let placeholder = ty.placeholder();
            let mut count = 0usize;
            current = re
                .replace_all(&current, |_caps: &regex::bytes::Captures| {
                    count += 1;
                    placeholder.to_vec()
                })
                .into_owned();
            if count > 0 {
                counts.insert(*ty, count);
            }
        }
        (current, counts)
    }
}
