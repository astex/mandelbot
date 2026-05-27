//! Haiku-driven model router. Given a user prompt, invoke `claude --model
//! haiku` non-interactively, ask it to classify the task, and return a
//! chosen model name (`opus`, `sonnet`, or `haiku`) plus a short reason.

use std::process::Command;

use serde::Deserialize;

use crate::tab::AgentRank;

const ROUTER_SYSTEM_PROMPT: &str = "You are a model router. Read the user's task and pick the best Claude model.\n\
\n\
Pick:\n\
- \"opus\" for complex reasoning, agentic coding across many files, deep analysis, legal/financial/scientific work, ambiguous or open-ended prompts.\n\
- \"sonnet\" for everyday development, document or large-codebase synthesis, orchestrator/worker pipelines, balanced default tasks.\n\
- \"haiku\" for simple classification, micro-tasks, basic translations, parallel subtasks, high-volume short-form work.\n\
\n\
Respond with EXACTLY one JSON object and NOTHING else:\n\
{\"model\": \"opus|sonnet|haiku\", \"reason\": \"<10 words or fewer>\"}\n\
No prose, no markdown code fences.";

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
    let output = Command::new("claude")
        .arg("--model")
        .arg("haiku")
        .arg("--output-format")
        .arg("json")
        .arg("--append-system-prompt")
        .arg(ROUTER_SYSTEM_PROMPT)
        .arg("-p")
        .arg(user_prompt)
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
    let doc: DecisionDoc = serde_json::from_str(stripped)
        .map_err(|e| RouterError::ParseDecision(e.to_string()))?;
    let model = doc.model.trim().to_ascii_lowercase();
    match model.as_str() {
        "opus" | "sonnet" | "haiku" => Ok(RouterDecision {
            model,
            reason: doc.reason.trim().to_string(),
        }),
        _ => Err(RouterError::UnknownModel(doc.model)),
    }
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
