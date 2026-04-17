use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::AgentRank;

/// Return the runtime directory for this process, using `$XDG_RUNTIME_DIR`
/// when available and falling back to `~/.mandelbot/run/`.
pub fn runtime_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(xdg)
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".mandelbot").join("run")
    }
    .join(format!("mandelbot-{}", std::process::id()))
}

/// Return the current executable path, stripping the " (deleted)" suffix that
/// Linux appends to `/proc/self/exe` when the binary has been replaced on disk
/// (e.g. after a rebuild while the app is still running).
fn current_exe_path() -> String {
    std::env::current_exe()
        .expect("failed to get current exe")
        .to_string_lossy()
        .trim_end_matches(" (deleted)")
        .to_owned()
}

pub fn create_fifo(path: &Path) {
    let c_path =
        std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
            .unwrap();
    let ret = unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        if err.kind() != std::io::ErrorKind::AlreadyExists {
            panic!("mkfifo({}) failed: {err}", path.display());
        }
    }
}

/// Write config files to a temp directory for Claude. Returns the directory
/// path. The MCP config and hooks settings are static — tab ID and parent
/// socket path are passed via environment variables so that every tab sees
/// the same commands and Claude only prompts for approval once.
pub(super) fn write_mcp_config() -> PathBuf {
    let dir = runtime_dir().join("mcp");
    let config_path = dir.join("mcp-config.json");

    if config_path.exists() {
        return dir;
    }

    std::fs::create_dir_all(&dir)
        .expect("failed to create mcp config dir");

    let exe = current_exe_path();

    let config = serde_json::json!({
        "mcpServers": {
            "mandelbot": {
                "command": exe,
                "args": ["--mcp-server"],
            },
        },
    });

    std::fs::write(
        &config_path,
        serde_json::to_string_pretty(&config).unwrap(),
    )
    .expect("failed to write mcp config");

    dir
}

pub(super) fn write_hooks_settings(dir: &Path) -> PathBuf {
    let path = dir.join("hooks-settings.json");

    let set_status = |status: &str| -> serde_json::Value {
        serde_json::json!({
            "type": "command",
            "command": format!("echo status:{status} > $MANDELBOT_FIFO"),
        })
    };

    // A conditional variant that only sets status when the tool_name is NOT
    // ExitPlanMode. This avoids a race between the catch-all "blocked" hook
    // and the ExitPlanMode-specific "needs_review" hook, which both fire in
    // parallel on an ExitPlanMode permission request.
    let set_status_unless_exit_plan =
        |status: &str| -> serde_json::Value {
            serde_json::json!({
                "type": "command",
                "command": format!(
                    r#"grep -q '"tool_name":"ExitPlanMode"\|"tool_name": "ExitPlanMode"' || echo status:{status} > $MANDELBOT_FIFO"#,
                ),
            })
        };

    let settings = serde_json::json!({
        "hooks": {
            "SessionStart": [{
                "hooks": [set_status("idle")],
            }],
            "UserPromptSubmit": [{
                "hooks": [
                    set_status("working"),
                    {
                        "type": "command",
                        "command": "echo checkpoint > $MANDELBOT_FIFO",
                    },
                ],
            }],
            "PreToolUse": [{
                "matcher": "",
                "hooks": [set_status("working")],
            }],
            "PermissionRequest": [
                {
                    "hooks": [set_status_unless_exit_plan("blocked")],
                },
                {
                    "matcher": "ExitPlanMode",
                    "hooks": [set_status("needs_review")],
                },
            ],
            "PostToolUse": [
                {
                    "matcher": "",
                    "hooks": [set_status("working")],
                },
                {
                    // Capture ScheduleWakeup tool calls so the tab can
                    // show a pending wake-up alongside backgrounded
                    // shells.  `tool_response.scheduledFor` is epoch ms
                    // (or 0 when the runtime declined to schedule, e.g.
                    // /loop dynamic disabled or the loop aged out).
                    // Parsed with grep instead of jq to avoid adding a
                    // runtime dep — the field is a top-level number in
                    // the hook's stdin JSON, so the regex is unambig.
                    "matcher": "ScheduleWakeup",
                    "hooks": [{
                        "type": "command",
                        "command": r#"sf=$(grep -oE '"scheduledFor":[0-9]+' | grep -oE '[0-9]+'); [ "${sf:-0}" -gt 0 ] && echo "wakeup_at:$sf" > $MANDELBOT_FIFO"#,
                    }],
                },
            ],
            "PostToolUseFailure": [{
                "hooks": [set_status("working")],
            }],
            "PreCompact": [{
                "hooks": [set_status("compacting")],
            }],
            "PostCompact": [{
                "hooks": [set_status("idle")],
            }],
            "Stop": [{
                "hooks": [set_status("idle")],
            }],
            "StopFailure": [{
                "hooks": [set_status("error")],
            }],
        },
    });

    std::fs::write(
        &path,
        serde_json::to_string_pretty(&settings).unwrap(),
    )
    .expect("failed to write hooks settings");

    path
}

const SYSTEM_PROMPT: &str =
    include_str!("../agents/PROMPT.md");
const SYSTEM_PROMPT_WORKTREE: &str =
    include_str!("../agents/PROMPT_worktree.md");
const HOME_PROMPT: &str =
    include_str!("../agents/HOME_PROMPT.md");
const PROJECT_PROMPT: &str =
    include_str!("../agents/PROJECT_PROMPT.md");

const SKILL_SHARED_COORD: &str =
    include_str!("../agents/skills/_shared/coord.md");
const SKILL_SHARED_INDEX_TEMPLATE: &str =
    include_str!("../agents/skills/_shared/index.template.md");
const SKILL_SHARED_CHILD_TEMPLATE: &str =
    include_str!("../agents/skills/_shared/child.template.md");
const SKILL_SHARED_WATCH: &str =
    include_str!("../agents/skills/_shared/watch.sh");
const SKILL_DELEGATE: &str =
    include_str!("../agents/skills/mandelbot-delegate/SKILL.md");
const SKILL_DELEGATE_NOGIT: &str =
    include_str!("../agents/skills/mandelbot-delegate/SKILL.nogit.md");
const SKILL_WORK_AS_SUBTASK: &str =
    include_str!("../agents/skills/mandelbot-work-as-subtask/SKILL.md");
const SKILL_WORK_AS_SUBTASK_NOGIT: &str =
    include_str!("../agents/skills/mandelbot-work-as-subtask/SKILL.nogit.md");
const SKILL_MANDELBOT_CONFIG: &str =
    include_str!("../agents/skills/mandelbot-config/SKILL.md");
const SKILL_MANDELBOT_KEYBINDINGS: &str =
    include_str!("../agents/skills/mandelbot-keybindings/SKILL.md");
const SKILL_MANDELBOT_FEATURES: &str =
    include_str!("../agents/skills/mandelbot-features/SKILL.md");
const SKILL_MANDELBOT_SPIKE_HARDEN: &str =
    include_str!("../agents/skills/mandelbot-spike-harden/SKILL.md");
const SKILL_MANDELBOT_IMPLEMENT_ITERATE: &str =
    include_str!("../agents/skills/mandelbot-implement-iterate/SKILL.md");
const SKILL_MANDELBOT_IMPLEMENT_ITERATE_NOGIT: &str =
    include_str!("../agents/skills/mandelbot-implement-iterate/SKILL.nogit.md");
const SKILL_MANDELBOT_IMPLEMENT_ITERATE_GENERATION: &str =
    include_str!("../agents/skills/mandelbot-implement-iterate/GENERATION.md");
const SKILL_MANDELBOT_TOURNAMENT: &str =
    include_str!("../agents/skills/mandelbot-tournament/SKILL.md");
const SKILL_MANDELBOT_ADVERSARIAL: &str =
    include_str!("../agents/skills/mandelbot-adversarial/SKILL.md");
const SKILL_MANDELBOT_GIT_MONITOR: &str =
    include_str!("../agents/skills/mandelbot-git-monitor/SKILL.md");
const SKILL_MANDELBOT_GIT_MONITOR_WATCH: &str =
    include_str!("../agents/skills/mandelbot-git-monitor/watch-prs.sh");

const SHELL_INTEGRATION_ZSH: &str = r#"
# Mandelbot shell integration — sets tab title to cwd + running command.
# \xc2\xa0 = UTF-8 non-breaking space, used as delimiter + visual spacing.
_mandelbot_prompt_char() { if (( EUID == 0 )); then printf '#'; else printf '%%'; fi }
_mandelbot_preexec() {
  printf '\e]0;%s\xc2\xa0%s\xc2\xa0%s\a' "${PWD/#$HOME/~}" "$(_mandelbot_prompt_char)" "$1"
  [ -n "$MANDELBOT_FIFO" ] && echo status:working > "$MANDELBOT_FIFO"
}
_mandelbot_precmd() {
  printf '\e]0;%s\xc2\xa0%s\xc2\xa0\a' "${PWD/#$HOME/~}" "$(_mandelbot_prompt_char)"
  [ -n "$MANDELBOT_FIFO" ] && echo status:idle > "$MANDELBOT_FIFO"
}
autoload -Uz add-zsh-hook
add-zsh-hook preexec _mandelbot_preexec
add-zsh-hook precmd  _mandelbot_precmd
"#;

const SHELL_INTEGRATION_BASH: &str = r#"
# Mandelbot shell integration — sets tab title to cwd + running command.
# \xc2\xa0 = UTF-8 non-breaking space, used as delimiter + visual spacing.
_mandelbot_prompt_char() { if [ "$EUID" = 0 ]; then printf '#'; else printf '$'; fi }
_mandelbot_preexec() {
  if [ -z "$MANDELBOT_IN_PROMPT" ]; then
    printf '\e]0;%s\xc2\xa0%s\xc2\xa0%s\a' "${PWD/#$HOME/\~}" "$(_mandelbot_prompt_char)" "$BASH_COMMAND"
    [ -n "$MANDELBOT_FIFO" ] && echo status:working > "$MANDELBOT_FIFO"
  fi
}
_mandelbot_precmd() {
  MANDELBOT_IN_PROMPT=1
  printf '\e]0;%s\xc2\xa0%s\xc2\xa0\a' "${PWD/#$HOME/\~}" "$(_mandelbot_prompt_char)"
  [ -n "$MANDELBOT_FIFO" ] && echo status:idle > "$MANDELBOT_FIFO"
  unset MANDELBOT_IN_PROMPT
}
trap '_mandelbot_preexec' DEBUG
PROMPT_COMMAND="_mandelbot_precmd${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
"#;

/// Write shell integration scripts and return env vars to source them.
pub(super) fn shell_integration_env(
    shell_command: &str,
) -> HashMap<String, String> {
    let dir = runtime_dir().join("shell");
    std::fs::create_dir_all(&dir)
        .expect("failed to create shell integration dir");

    let mut env = HashMap::new();
    env.insert(
        "TERM_PROGRAM".to_string(),
        "mandelbot".to_string(),
    );

    if shell_command.contains("zsh") {
        let path = dir.join("mandelbot.zsh");
        std::fs::write(&path, SHELL_INTEGRATION_ZSH)
            .expect("failed to write zsh integration");
        // ZDOTDIR trick: create a .zshrc that sources the user's real
        // config then ours.
        let zdotdir = dir.join("zdotdir");
        std::fs::create_dir_all(&zdotdir)
            .expect("failed to create zdotdir");
        let user_home =
            std::env::var("HOME").unwrap_or_default();
        let zshrc = zdotdir.join(".zshrc");
        let content = format!(
            "[ -f \"{user_home}/.zshenv\" ] && source \"{user_home}/.zshenv\"\n\
             [ -f \"{user_home}/.zshrc\" ] && source \"{user_home}/.zshrc\"\n\
             source \"{}\"\n",
            path.to_string_lossy()
        );
        std::fs::write(&zshrc, content)
            .expect("failed to write zdotdir .zshrc");
        // Also create .zshenv to prevent double-sourcing of
        // /etc/zshenv via ZDOTDIR.
        let zshenv = zdotdir.join(".zshenv");
        if !zshenv.exists() {
            std::fs::write(&zshenv, "")
                .expect("failed to write zdotdir .zshenv");
        }
        env.insert(
            "ZDOTDIR".to_string(),
            zdotdir.to_string_lossy().into_owned(),
        );
    } else if shell_command.contains("bash") {
        let path = dir.join("mandelbot.bash");
        std::fs::write(&path, SHELL_INTEGRATION_BASH)
            .expect("failed to write bash integration");
        // For bash, use --rcfile or ENV. We'll set ENV for
        // non-login shells. Since we source user's bashrc too,
        // write a wrapper.
        let wrapper = dir.join("bashrc_wrapper");
        let user_home =
            std::env::var("HOME").unwrap_or_default();
        let content = format!(
            "[ -f \"{user_home}/.bashrc\" ] && source \"{user_home}/.bashrc\"\n\
             source \"{}\"\n",
            path.to_string_lossy()
        );
        std::fs::write(&wrapper, content)
            .expect("failed to write bash wrapper");
        env.insert(
            "ENV".to_string(),
            wrapper.to_string_lossy().into_owned(),
        );
    }

    env
}

pub(super) fn write_system_prompt(
    dir: &Path,
    rank: AgentRank,
    tab_id: usize,
    workflow: &str,
) -> PathBuf {
    let (filename, base_content) = match rank {
        AgentRank::Home => {
            (format!("home-prompt-{tab_id}.md"), HOME_PROMPT.to_string())
        }
        AgentRank::Project => {
            (format!("project-prompt-{tab_id}.md"), PROJECT_PROMPT.to_string())
        }
        AgentRank::Task => {
            let mut content = SYSTEM_PROMPT.to_string();
            if workflow == "git" {
                content.push_str(SYSTEM_PROMPT_WORKTREE);
            }
            (format!("system-prompt-{tab_id}.md"), content)
        }
    };
    let path = dir.join(filename);
    if !path.exists() {
        let content = format!(
            "{base_content}\n\
             <system-reminder>\n\
             Your mandelbot tab ID is {tab_id}. \
             You can close yourself or any of your child tabs \
             using the mandelbot MCP close_tab tool.\n\
             </system-reminder>\n"
        );
        std::fs::write(&path, content)
            .expect("failed to write system prompt");
    }
    path
}

pub(super) fn write_plugin_dir(
    dir: &Path,
    workflow: &str,
) -> PathBuf {
    let plugin_dir = dir.join("plugins");

    let shared_dir = plugin_dir.join("skills").join("_shared");
    std::fs::create_dir_all(&shared_dir)
        .expect("failed to create _shared skill dir");

    let delegate_dir =
        plugin_dir.join("skills").join("mandelbot-delegate");
    std::fs::create_dir_all(&delegate_dir)
        .expect("failed to create mandelbot-delegate skill dir");

    let subtask_dir = plugin_dir
        .join("skills")
        .join("mandelbot-work-as-subtask");
    std::fs::create_dir_all(&subtask_dir).expect(
        "failed to create mandelbot-work-as-subtask skill dir",
    );

    let config_dir =
        plugin_dir.join("skills").join("mandelbot-config");
    std::fs::create_dir_all(&config_dir)
        .expect("failed to create mandelbot-config skill dir");

    let keybindings_dir =
        plugin_dir.join("skills").join("mandelbot-keybindings");
    std::fs::create_dir_all(&keybindings_dir).expect(
        "failed to create mandelbot-keybindings skill dir",
    );

    let features_dir =
        plugin_dir.join("skills").join("mandelbot-features");
    std::fs::create_dir_all(&features_dir)
        .expect("failed to create mandelbot-features skill dir");

    let spike_harden_dir = plugin_dir
        .join("skills")
        .join("mandelbot-spike-harden");
    std::fs::create_dir_all(&spike_harden_dir).expect(
        "failed to create mandelbot-spike-harden skill dir",
    );

    let delegate_content = if workflow == "git" {
        SKILL_DELEGATE
    } else {
        SKILL_DELEGATE_NOGIT
    };
    std::fs::write(shared_dir.join("coord.md"), SKILL_SHARED_COORD)
        .expect("failed to write shared coord protocol");
    std::fs::write(
        shared_dir.join("index.template.md"),
        SKILL_SHARED_INDEX_TEMPLATE,
    )
    .expect("failed to write shared index template");
    std::fs::write(
        shared_dir.join("child.template.md"),
        SKILL_SHARED_CHILD_TEMPLATE,
    )
    .expect("failed to write shared child template");
    std::fs::write(shared_dir.join("watch.sh"), SKILL_SHARED_WATCH)
        .expect("failed to write shared watch script");

    let skill_path = delegate_dir.join("SKILL.md");
    std::fs::write(&skill_path, delegate_content)
        .expect("failed to write delegate skill");

    let subtask_content = if workflow == "git" {
        SKILL_WORK_AS_SUBTASK
    } else {
        SKILL_WORK_AS_SUBTASK_NOGIT
    };
    let skill_path = subtask_dir.join("SKILL.md");
    std::fs::write(&skill_path, subtask_content)
        .expect("failed to write work-as-subtask skill");

    let skill_path = config_dir.join("SKILL.md");
    if !skill_path.exists() {
        std::fs::write(&skill_path, SKILL_MANDELBOT_CONFIG)
            .expect("failed to write mandelbot-config skill");
    }

    let skill_path = keybindings_dir.join("SKILL.md");
    if !skill_path.exists() {
        std::fs::write(
            &skill_path,
            SKILL_MANDELBOT_KEYBINDINGS,
        )
        .expect("failed to write mandelbot-keybindings skill");
    }

    let skill_path = features_dir.join("SKILL.md");
    if !skill_path.exists() {
        std::fs::write(&skill_path, SKILL_MANDELBOT_FEATURES)
            .expect("failed to write mandelbot-features skill");
    }

    let skill_path = spike_harden_dir.join("SKILL.md");
    if !skill_path.exists() {
        std::fs::write(
            &skill_path,
            SKILL_MANDELBOT_SPIKE_HARDEN,
        )
        .expect("failed to write mandelbot-spike-harden skill");
    }

    let implement_iterate_dir = plugin_dir
        .join("skills")
        .join("mandelbot-implement-iterate");
    std::fs::create_dir_all(&implement_iterate_dir).expect(
        "failed to create mandelbot-implement-iterate skill dir",
    );

    let iterate_content = if workflow == "git" {
        SKILL_MANDELBOT_IMPLEMENT_ITERATE
    } else {
        SKILL_MANDELBOT_IMPLEMENT_ITERATE_NOGIT
    };
    let skill_path = implement_iterate_dir.join("SKILL.md");
    std::fs::write(&skill_path, iterate_content)
        .expect("failed to write mandelbot-implement-iterate skill");
    std::fs::write(
        implement_iterate_dir.join("GENERATION.md"),
        SKILL_MANDELBOT_IMPLEMENT_ITERATE_GENERATION,
    )
    .expect("failed to write mandelbot-implement-iterate generation protocol");

    if workflow == "git" {
        let tournament_dir =
            plugin_dir.join("skills").join("mandelbot-tournament");
        std::fs::create_dir_all(&tournament_dir)
            .expect("failed to create mandelbot-tournament skill dir");
        std::fs::write(
            tournament_dir.join("SKILL.md"),
            SKILL_MANDELBOT_TOURNAMENT,
        )
        .expect("failed to write mandelbot-tournament skill");

        let adversarial_dir =
            plugin_dir.join("skills").join("mandelbot-adversarial");
        std::fs::create_dir_all(&adversarial_dir)
            .expect("failed to create mandelbot-adversarial skill dir");
        std::fs::write(
            adversarial_dir.join("SKILL.md"),
            SKILL_MANDELBOT_ADVERSARIAL,
        )
        .expect("failed to write mandelbot-adversarial skill");

        let git_monitor_dir =
            plugin_dir.join("skills").join("mandelbot-git-monitor");
        std::fs::create_dir_all(&git_monitor_dir)
            .expect("failed to create mandelbot-git-monitor skill dir");
        std::fs::write(
            git_monitor_dir.join("SKILL.md"),
            SKILL_MANDELBOT_GIT_MONITOR,
        )
        .expect("failed to write mandelbot-git-monitor skill");
        std::fs::write(
            git_monitor_dir.join("watch-prs.sh"),
            SKILL_MANDELBOT_GIT_MONITOR_WATCH,
        )
        .expect("failed to write mandelbot-git-monitor watch script");
    }

    plugin_dir
}
