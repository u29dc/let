#![forbid(unsafe_code)]

use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvValueSource {
    Process,
    EnvFile,
}

pub fn resolve_env_var(key: &str, env_file: &Path) -> Option<(String, EnvValueSource)> {
    if let Ok(value) = std::env::var(key) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some((trimmed.to_owned(), EnvValueSource::Process));
        }
    }

    read_env_file_var(key, env_file).map(|value| (value, EnvValueSource::EnvFile))
}

fn read_env_file_var(key: &str, env_file: &Path) -> Option<String> {
    let content = fs::read_to_string(env_file).ok()?;
    let mut found = None;
    for line in content.lines() {
        if let Some((candidate_key, candidate_value)) = parse_env_line(line)
            && candidate_key == key
            && !candidate_value.trim().is_empty()
        {
            found = Some(candidate_value);
        }
    }
    found
}

fn parse_env_line(line: &str) -> Option<(&str, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    let assignment = trimmed.strip_prefix("export ").unwrap_or(trimmed);
    let (raw_key, raw_value) = assignment.split_once('=')?;
    let key = raw_key.trim();
    if key.is_empty() {
        return None;
    }

    Some((key, parse_env_value(raw_value.trim())))
}

fn parse_env_value(raw: &str) -> String {
    if raw.len() >= 2 {
        let starts_single = raw.starts_with('\'');
        let starts_double = raw.starts_with('"');
        if (starts_single && raw.ends_with('\'')) || (starts_double && raw.ends_with('"')) {
            return raw[1..raw.len() - 1].to_owned();
        }
    }

    let mut output = String::with_capacity(raw.len());
    let mut prev_was_whitespace = true;
    for ch in raw.chars() {
        if ch == '#' && prev_was_whitespace {
            break;
        }
        output.push(ch);
        prev_was_whitespace = ch.is_whitespace();
    }
    output.trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use super::{parse_env_line, parse_env_value};

    #[test]
    fn parse_env_line_skips_comments_and_blank_lines() {
        assert!(parse_env_line("").is_none());
        assert!(parse_env_line("   ").is_none());
        assert!(parse_env_line("# comment").is_none());
    }

    #[test]
    fn parse_env_line_supports_export_prefix() {
        let parsed = parse_env_line("export LET_KEY=value").expect("line should parse");
        assert_eq!(parsed.0, "LET_KEY");
        assert_eq!(parsed.1, "value");
    }

    #[test]
    fn parse_env_value_handles_quotes_and_inline_comments() {
        assert_eq!(parse_env_value("'abc 123'"), "abc 123");
        assert_eq!(parse_env_value("\"abc 123\""), "abc 123");
        assert_eq!(parse_env_value("abc # note"), "abc");
        assert_eq!(parse_env_value("abc#def"), "abc#def");
    }
}
