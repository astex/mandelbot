//! Haiku-driven model router. Given a user prompt, invoke `claude --model
//! haiku` non-interactively, ask it to classify the task, and return a
//! chosen model name (`opus`, `sonnet`, or `haiku`) plus a short reason.

use std::process::Command;

use serde::Deserialize;

use crate::tab::AgentRank;

// Wrapping the routing instructions into the user message rather than the
// system prompt: `claude --append-system-prompt` stacks on top of Claude
// Code's default persona, which buries short routing instructions. Putting
// the directive in the user message keeps it as the foreground task.
const ROUTER_PREAMBLE: &str = "You are a model router. DO NOT perform the task described below — only classify it.\n\
\n\
Pick exactly one model:\n\
- \"opus\": complex reasoning, agentic coding across many files, deep analysis, legal/financial/scientific work, ambiguous or open-ended prompts.\n\
- \"sonnet\": everyday development, document or large-codebase synthesis, orchestrator/worker pipelines, balanced default tasks.\n\
- \"haiku\": simple classification, micro-tasks, basic translations, parallel subtasks, high-volume short-form work.\n\
\n\
Reply with EXACTLY one JSON object and NOTHING else:\n\
{\"model\": \"opus|sonnet|haiku\", \"reason\": \"<10 words or fewer>\"}\n\
No prose, no markdown code fences, no leading or trailing text.\n\
\n\
TASK TO CLASSIFY (do not execute, only classify):\n\
---\n";

#[derive(Debug, Clone)]
pub struct RouterDecision {
    pub model: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub enum RouterError {
    Spawn(String),
    NonZeroExit(i32),
    ParseEnvelope(String),
    ParseDecision(String),
    UnknownModel(String),
}

impl std::fmt::Display for RouterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(e) => write!(f, "claude spawn failed: {e}"),
            Self::NonZeroExit(c) => write!(f, "classifier exited with code {c}"),
            Self::ParseEnvelope(e) => write!(f, "could not parse claude envelope: {e}"),
            Self::ParseDecision(e) => write!(f, "could not parse router decision: {e}"),
            Self::UnknownModel(m) => write!(f, "router returned unknown model: {m}"),
        }
    }
}

/// Hard fallback when "auto" is requested but classification can't run or
/// the spawn site can't go through the async router (e.g. UI key bindings
/// with no prompt).
pub fn fallback_for_rank(rank: AgentRank) -> &'static str {
    match rank {
        AgentRank::Home => "haiku",
        AgentRank::Project => "sonnet",
        AgentRank::Task => "opus",
    }
}

/// Blocking classifier. Intended to be called from a worker thread via
/// `spawn_blocking_task` — iced 0.14's `Task::perform` doesn't bind to
/// the tokio runtime so we stay on `std::process::Command` here.
pub fn classify_blocking(user_prompt: &str) -> Result<RouterDecision, RouterError> {
    let wrapped = format!("{ROUTER_PREAMBLE}{user_prompt}\n---\n");
    let output = Command::new("claude")
        .arg("--model")
        .arg("haiku")
        .arg("--output-format")
        .arg("json")
        .arg("-p")
        .arg(&wrapped)
        .output()
        .map_err(|e| RouterError::Spawn(e.to_string()))?;

    if !output.status.success() {
        return Err(RouterError::NonZeroExit(
            output.status.code().unwrap_or(-1),
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let envelope: ClaudeEnvelope = serde_json::from_str(&stdout)
        .map_err(|e| RouterError::ParseEnvelope(e.to_string()))?;
    let raw = envelope.result.ok_or_else(|| {
        RouterError::ParseEnvelope("envelope missing `result` field".into())
    })?;
    parse_decision(&raw)
}

fn parse_decision(raw: &str) -> Result<RouterDecision, RouterError> {
    let stripped = strip_code_fence(raw.trim());
    // Try strict parse first; if Haiku wraps prose around the JSON,
    // fall back to extracting the first `{...}` substring.
    let doc: DecisionDoc = match serde_json::from_str(stripped) {
        Ok(d) => d,
        Err(strict_err) => match extract_first_json_object(stripped) {
            Some(snippet) => serde_json::from_str(snippet)
                .map_err(|e| RouterError::ParseDecision(e.to_string()))?,
            None => return Err(RouterError::ParseDecision(strict_err.to_string())),
        },
    };
    let model = doc.model.trim().to_ascii_lowercase();
    match model.as_str() {
        "opus" | "sonnet" | "haiku" => Ok(RouterDecision {
            model,
            reason: doc.reason.trim().to_string(),
        }),
        _ => Err(RouterError::UnknownModel(doc.model)),
    }
}

/// Walk the string looking for the first balanced `{...}` block.  Doesn't
/// try to be a real JSON tokenizer — it's good enough to fish a decision
/// object out of stray prose since `{` and `}` characters are unusual in
/// English.
fn extract_first_json_object(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    let start = bytes.iter().position(|&b| b == b'{')?;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[start..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Strip a ```...``` or ```json...``` fence if Haiku decides to wrap its
/// answer despite instructions to the contrary.
fn strip_code_fence(s: &str) -> &str {
    let s = s.trim();
    let rest = s
        .strip_prefix("```json")
        .or_else(|| s.strip_prefix("```"))
        .unwrap_or(s)
        .trim();
    rest.strip_suffix("```").unwrap_or(rest).trim()
}

#[derive(Deserialize)]
struct ClaudeEnvelope {
    result: Option<String>,
}

#[derive(Deserialize)]
struct DecisionDoc {
    model: String,
    #[serde(default)]
    reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_clean_json() {
        let d = parse_decision(r#"{"model": "opus", "reason": "complex refactor"}"#).unwrap();
        assert_eq!(d.model, "opus");
        assert_eq!(d.reason, "complex refactor");
    }

    #[test]
    fn parses_fenced_json() {
        let d = parse_decision("```json\n{\"model\":\"sonnet\",\"reason\":\"everyday dev\"}\n```")
            .unwrap();
        assert_eq!(d.model, "sonnet");
        assert_eq!(d.reason, "everyday dev");
    }

    #[test]
    fn parses_bare_fenced_json() {
        let d = parse_decision("```\n{\"model\":\"haiku\",\"reason\":\"micro\"}\n```").unwrap();
        assert_eq!(d.model, "haiku");
    }

    #[test]
    fn normalizes_model_case() {
        let d = parse_decision(r#"{"model":"Haiku","reason":"micro task"}"#).unwrap();
        assert_eq!(d.model, "haiku");
    }

    #[test]
    fn rejects_unknown_model() {
        match parse_decision(r#"{"model":"gpt-4","reason":"x"}"#) {
            Err(RouterError::UnknownModel(m)) => assert_eq!(m, "gpt-4"),
            other => panic!("expected UnknownModel, got {other:?}"),
        }
    }

    #[test]
    fn rejects_malformed_json() {
        assert!(matches!(
            parse_decision("this is not json"),
            Err(RouterError::ParseDecision(_))
        ));
    }

    #[test]
    fn extracts_object_buried_in_prose() {
        let d = parse_decision("Sure! Here you go: {\"model\":\"opus\",\"reason\":\"complex\"} hope that helps").unwrap();
        assert_eq!(d.model, "opus");
        assert_eq!(d.reason, "complex");
    }

    #[test]
    fn tolerates_missing_reason() {
        let d = parse_decision(r#"{"model":"opus"}"#).unwrap();
        assert_eq!(d.model, "opus");
        assert_eq!(d.reason, "");
    }

    #[test]
    fn fallback_table() {
        assert_eq!(fallback_for_rank(AgentRank::Home), "haiku");
        assert_eq!(fallback_for_rank(AgentRank::Project), "sonnet");
        assert_eq!(fallback_for_rank(AgentRank::Task), "opus");
    }
}
