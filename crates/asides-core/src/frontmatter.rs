//! The YAML front matter written once, when a note is created.

use std::fmt::Write;
use std::path::PathBuf;

use chrono::{DateTime, Local};

use crate::project::GitInfo;

#[derive(Debug, Clone)]
pub struct Metadata {
    pub title: String,
    pub created: DateTime<Local>,
    pub directory: PathBuf,
    pub git: Option<GitInfo>,
    pub macos: Option<String>,
    pub browser: Option<String>,
    pub display: Option<String>,
    pub transcription: String,
}

pub fn render(meta: &Metadata) -> String {
    let mut out = String::from("---\n");
    field(&mut out, 0, "title", Some(&meta.title));
    let _ = writeln!(out, "created: {}", meta.created.format("%Y-%m-%dT%H:%M:%S%:z"));
    field(&mut out, 0, "directory", Some(&meta.directory.display().to_string()));
    if let Some(git) = &meta.git {
        out.push_str("git:\n");
        field(&mut out, 1, "remote", git.remote.as_deref());
        field(&mut out, 1, "branch", git.branch.as_deref());
        field(&mut out, 1, "commit", git.commit.as_deref());
        field(&mut out, 1, "commit_subject", git.commit_subject.as_deref());
        let _ = writeln!(out, "  uncommitted_files: {}", git.uncommitted_files);
    }
    if meta.macos.is_some() || meta.browser.is_some() || meta.display.is_some() {
        out.push_str("environment:\n");
        field(&mut out, 1, "macos", meta.macos.as_deref());
        field(&mut out, 1, "browser", meta.browser.as_deref());
        field(&mut out, 1, "display", meta.display.as_deref());
    }
    field(&mut out, 0, "transcription", Some(&meta.transcription));
    out.push_str("---\n");
    out
}

fn field(out: &mut String, indent: usize, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        let _ = writeln!(out, "{}{key}: {}", "  ".repeat(indent), yaml_str(value));
    }
}

/// A YAML scalar, quoted only when a plain scalar would be misread.
fn yaml_str(s: &str) -> String {
    let lower = s.to_ascii_lowercase();
    let reserved = ["true", "false", "yes", "no", "on", "off", "null", "~", ""].contains(&lower.as_str());
    let numeric = s.parse::<f64>().is_ok();
    let special_start = s.starts_with(|c: char| "-?:,[]{}#&*!|>'\"%@`".contains(c) || c.is_whitespace());
    let special_inside = s.contains(':') || s.contains(" #") || s.contains('\n') || s.ends_with(' ');
    if reserved || numeric || special_start || special_inside {
        let escaped = s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
        format!("\"{escaped}\"")
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn renders_full_metadata() {
        let meta = Metadata {
            title: "Checkout session".into(),
            created: Local.with_ymd_and_hms(2026, 10, 15, 14, 32, 8).unwrap(),
            directory: "/Users/me/code/shop".into(),
            git: Some(GitInfo {
                remote: Some("git@github.com:acme/shop.git".into()),
                branch: Some("feat/checkout".into()),
                commit: Some("1234567".into()),
                commit_subject: Some("Fix: \"rounding\"".into()),
                uncommitted_files: 3,
            }),
            macos: Some("26.5.2".into()),
            browser: Some("Firefox 143.0".into()),
            display: Some("3024x1964 @2x".into()),
            transcription: "asides 0.1.0 · whisper large-v3-turbo (local)".into(),
        };
        let offset = meta.created.format("%:z").to_string();
        assert_eq!(
            render(&meta),
            format!(
                "---\ntitle: Checkout session\ncreated: 2026-10-15T14:32:08{offset}\ndirectory: /Users/me/code/shop\n\
                 git:\n  remote: \"git@github.com:acme/shop.git\"\n  branch: feat/checkout\n  commit: \"1234567\"\n  \
                 commit_subject: \"Fix: \\\"rounding\\\"\"\n  uncommitted_files: 3\n\
                 environment:\n  macos: 26.5.2\n  browser: Firefox 143.0\n  display: 3024x1964 @2x\n\
                 transcription: asides 0.1.0 · whisper large-v3-turbo (local)\n---\n"
            )
        );
    }

    #[test]
    fn omits_missing_sections() {
        let meta = Metadata {
            title: "yes".into(),
            created: Local.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap(),
            directory: "/tmp/x".into(),
            git: None,
            macos: None,
            browser: None,
            display: None,
            transcription: "t".into(),
        };
        let out = render(&meta);
        assert!(out.starts_with("---\ntitle: \"yes\"\n"), "{out}");
        assert!(!out.contains("git:") && !out.contains("environment:"));
    }
}
