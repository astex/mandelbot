use std::collections::HashMap;
use std::io::{BufRead, Read, Write};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::vte::ansi;
use portable_pty::PtySize;

use futures::SinkExt;

use super::config;
use super::{
    detect_prompt_pr_number, detect_prompt_shell_count,
    AgentStatus, TabEvent, TabSpawnParams, TermEventListener,
    TermInstance,
};
use crate::pty;
use crate::ui::Message;
use crate::worktree;

/// Read status updates from a FIFO and emit `SetStatus` messages.
/// Opens the FIFO with O_RDWR to avoid EOF when writers close.
pub fn fifo_stream(
    tab_id: usize,
    fifo_path: PathBuf,
) -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(
        16,
        move |mut sender: iced::futures::channel::mpsc::Sender<
            Message,
        >| async move {
            let (exit_sender, exit_receiver) =
                iced::futures::channel::oneshot::channel::<()>();

            std::thread::spawn(move || {
                // Open O_RDWR so the read side stays open even when
                // no writers are connected (avoids repeated EOF).
                let file = match std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&fifo_path)
                {
                    Ok(f) => f,
                    Err(_) => return,
                };
                let reader = std::io::BufReader::new(file);
                for line in reader.lines() {
                    let Ok(line) = line else { break };
                    let trimmed = line.trim();
                    if let Some(s) = trimmed
                        .strip_prefix("status:")
                        .and_then(AgentStatus::from_str)
                    {
                        if futures::executor::block_on(
                            sender.send(Message::SetStatus(
                                tab_id, s,
                            )),
                        )
                        .is_err()
                        {
                            break;
                        }
                    } else if trimmed == "checkpoint" {
                        if futures::executor::block_on(
                            sender.send(Message::AutoCheckpoint(
                                tab_id,
                            )),
                        )
                        .is_err()
                        {
                            break;
                        }
                    } else if let Some(ts) = trimmed
                        .strip_prefix("wakeup_at:")
                        .and_then(|s| s.parse::<u64>().ok())
                    {
                        if futures::executor::block_on(
                            sender.send(Message::WakeupAt(
                                tab_id, ts,
                            )),
                        )
                        .is_err()
                        {
                            break;
                        }
                    }
                }
                let _ = exit_sender.send(());
            });

            let _ = exit_receiver.await;
        },
    )
}

pub fn tab_stream(
    params: TabSpawnParams,
    event_rx: mpsc::Receiver<TabEvent>,
    pty_event_tx: mpsc::Sender<TabEvent>,
    term: Arc<Mutex<TermInstance>>,
    listener: TermEventListener,
) -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(
        32,
        move |mut sender: iced::futures::channel::mpsc::Sender<
            Message,
        >| async move {
            let (exit_sender, exit_receiver) =
                iced::futures::channel::oneshot::channel::<()>();

            std::thread::spawn(move || {
                let id = params.id;
                let is_claude = params.is_claude;
                let rank = params.rank;
                let project_dir = params.project_dir;
                let control_prefix = params.control_prefix;

                // --- Setup phase ---

                let workflow = if params.workflow == "detect" {
                    let is_git =
                        project_dir.as_ref().is_some_and(|dir| {
                            std::process::Command::new("git")
                                .args([
                                    "rev-parse",
                                    "--is-inside-work-tree",
                                ])
                                .current_dir(dir)
                                .stdout(
                                    std::process::Stdio::null(),
                                )
                                .stderr(
                                    std::process::Stdio::null(),
                                )
                                .status()
                                .is_ok_and(|s| s.success())
                        });
                    if is_git {
                        "git".to_string()
                    } else {
                        "none".to_string()
                    }
                } else {
                    params.workflow
                };

                let config_dir = config::write_mcp_config();
                let mcp_config_flag = config_dir
                    .join("mcp-config.json")
                    .to_string_lossy()
                    .into_owned();
                let home =
                    std::env::var("HOME").unwrap_or_default();
                let mandelbot_dir =
                    PathBuf::from(&home).join(".mandelbot");
                let system_prompt_path =
                    config::write_system_prompt(
                        &config_dir, rank, id, &workflow,
                    );
                let system_prompt_flag = system_prompt_path
                    .to_string_lossy()
                    .into_owned();
                let hooks_settings_path =
                    config::write_hooks_settings(&config_dir);
                let hooks_settings_flag = hooks_settings_path
                    .to_string_lossy()
                    .into_owned();

                let prompt_flag =
                    params.prompt.unwrap_or_default();
                let command: String;
                let args_owned: Vec<String>;
                let mut env: HashMap<String, String>;
                let cwd: Option<PathBuf>;
                let worktree_dir: Option<PathBuf>;

                if is_claude {
                    let shell_parts: Vec<&str> =
                        params.shell.split_whitespace().collect();
                    command = shell_parts[0].to_string();

                    let mut claude_args = format!(
                        "claude --model {} --mcp-config {} \
                         --append-system-prompt-file {} \
                         --settings {}",
                        pty::shell_quote(&params.model),
                        pty::shell_quote(&mcp_config_flag),
                        pty::shell_quote(&system_prompt_flag),
                        pty::shell_quote(&hooks_settings_flag),
                    );

                    let setup_script;
                    (setup_script, worktree_dir) =
                        if let Some(existing) = params.existing_worktree.clone() {
                            // replace/fork flow: the worktree was pre-created
                            // by the time-travel handlers. Still mirror
                            // `.claude/settings.local.json` from the project
                            // root so the new tab inherits local settings.
                            let wt_str = existing.to_string_lossy();
                            let copy = project_dir
                                .as_ref()
                                .map(|dir| worktree::copy_settings_snippet(dir, &existing))
                                .unwrap_or_default();
                            let script = if copy.is_empty() {
                                format!("cd {}", pty::shell_quote(&wt_str))
                            } else {
                                format!(
                                    "{copy} && cd {}",
                                    pty::shell_quote(&wt_str),
                                )
                            };
                            (script, Some(existing))
                        } else if rank == super::AgentRank::Task
                            && workflow == "git"
                            && let Some(dir) =
                                project_dir.as_ref()
                        {
                            let (script, path) =
                                worktree::setup_script(
                                    dir,
                                    &params.worktree_location,
                                    params.branch.as_deref(),
                                    params.base.as_deref(),
                                );
                            (script, Some(path))
                        } else {
                            (String::new(), None)
                        };

                    let plugin_dir = config::write_plugin_dir(
                        &config_dir, &workflow,
                    );
                    claude_args.push_str(&format!(
                        " --plugin-dir {}",
                        pty::shell_quote(
                            &plugin_dir.to_string_lossy()
                        ),
                    ));
                    claude_args.push_str(&format!(
                        " --add-dir {}",
                        pty::shell_quote(
                            &mandelbot_dir.to_string_lossy()
                        ),
                    ));
                    if let Some(sid) = params.resume_session_id.as_deref() {
                        claude_args.push_str(&format!(
                            " --resume {}",
                            pty::shell_quote(sid),
                        ));
                    } else if let Some(sid) = params.session_id.as_deref() {
                        claude_args.push_str(&format!(
                            " --session-id {}",
                            pty::shell_quote(sid),
                        ));
                    }
                    if !prompt_flag.is_empty() {
                        claude_args.push_str(" -- ");
                        claude_args.push_str(
                            &pty::shell_quote(&prompt_flag),
                        );
                    }

                    let wrapped_cmd = if setup_script.is_empty()
                    {
                        format!("exec {claude_args}")
                    } else {
                        format!(
                            "{setup_script} && exec {claude_args}"
                        )
                    };

                    let fifo_path = config::runtime_dir()
                        .join(format!("{id}.fifo"));
                    env = HashMap::from([
                        (
                            "MANDELBOT_TAB_ID".to_string(),
                            id.to_string(),
                        ),
                        (
                            "MANDELBOT_PARENT_SOCKET".to_string(),
                            params
                                .parent_socket
                                .to_string_lossy()
                                .into_owned(),
                        ),
                        (
                            "MANDELBOT_FIFO".to_string(),
                            fifo_path
                                .to_string_lossy()
                                .into_owned(),
                        ),
                    ]);

                    cwd = if !setup_script.is_empty() {
                        project_dir.clone()
                    } else {
                        worktree_dir
                            .clone()
                            .or(project_dir.clone())
                    };

                    args_owned = vec![
                        "-l".to_string(),
                        "-i".to_string(),
                        "-c".to_string(),
                        wrapped_cmd,
                    ];
                } else {
                    worktree_dir = None;
                    let parts: Vec<&str> =
                        params.shell.split_whitespace().collect();
                    let (cmd, rest) = parts
                        .split_first()
                        .expect("shell config must not be empty");
                    command = cmd.to_string();
                    args_owned = rest
                        .iter()
                        .map(|s| s.to_string())
                        .collect();
                    let fifo_path = config::runtime_dir()
                        .join(format!("{id}.fifo"));
                    env = config::shell_integration_env(&command);
                    env.insert(
                        "MANDELBOT_FIFO".to_string(),
                        fifo_path
                            .to_string_lossy()
                            .into_owned(),
                    );
                    cwd = None;
                }

                // Tell the UI about the worktree + session so
                // checkpoint/replace/fork can find the jsonl + repo.
                if is_claude {
                    let session_id = params
                        .resume_session_id
                        .clone()
                        .or_else(|| params.session_id.clone());
                    let _ = futures::executor::block_on(sender.send(
                        Message::TabReady {
                            tab_id: id,
                            worktree_dir: worktree_dir.clone(),
                            session_id,
                        },
                    ));
                }

                let args_refs: Vec<&str> =
                    args_owned.iter().map(|s| s.as_str()).collect();
                let shell_config = pty::ShellConfig {
                    command: &command,
                    args: &args_refs,
                    env,
                    cwd: cwd.as_deref(),
                    rows: params.rows as u16,
                    cols: params.cols as u16,
                };

                let (master, mut child) =
                    pty::spawn_shell(&shell_config)
                        .expect("failed to spawn PTY");
                let reader = master
                    .try_clone_reader()
                    .expect("failed to clone reader");
                let mut writer = master
                    .take_writer()
                    .expect("failed to take writer");

                // --- Spawn PTY reader thread ---
                std::thread::spawn(move || {
                    let mut reader = reader;
                    let mut buf = [0u8; 4096];
                    loop {
                        match reader.read(&mut buf) {
                            Ok(0) | Err(_) => {
                                let _ = pty_event_tx
                                    .send(TabEvent::PtyEof);
                                break;
                            }
                            Ok(n) => {
                                let _ =
                                    pty_event_tx.send(
                                        TabEvent::PtyData(
                                            buf[..n].to_vec(),
                                        ),
                                    );
                            }
                        }
                    }
                });

                // --- Main event loop ---
                let mut parser =
                    ansi::Processor::<ansi::StdSyncHandler>::new();
                let mut pty_cols = params.cols;
                let mut pty_alive = true;
                let mut bg_tasks: usize = 0;
                let mut pr_number: Option<u32> = None;

                loop {
                    match event_rx.recv() {
                        Ok(TabEvent::PtyData(bytes)) => {
                            let mut t = term.lock().unwrap();
                            let was_at_bottom =
                                t.grid().display_offset() == 0;
                            parser.advance(&mut *t, &bytes);
                            if was_at_bottom {
                                t.scroll_display(Scroll::Bottom);
                            }
                            let responses: Vec<String> = listener
                                .pty_responses
                                .lock()
                                .unwrap()
                                .drain(..)
                                .collect();
                            drop(t);
                            for response in responses {
                                let _ = writer.write_all(
                                    response.as_bytes(),
                                );
                                let _ = writer.flush();
                            }
                            if is_claude {
                                let t = term.lock().unwrap();
                                if let Some(count) =
                                    detect_prompt_shell_count(&t)
                                {
                                    bg_tasks = count;
                                }
                                pr_number =
                                    detect_prompt_pr_number(&t);
                            }
                            let _ =
                                futures::executor::block_on(
                                    sender.send(
                                        Message::TabOutput(
                                            id,
                                            bg_tasks,
                                            pr_number,
                                        ),
                                    ),
                                );
                        }
                        Ok(TabEvent::Input(bytes)) => {
                            if pty_alive {
                                let _ =
                                    writer.write_all(&bytes);
                                let _ = writer.flush();
                            }
                        }
                        Ok(TabEvent::Resize {
                            rows,
                            cols,
                            pixel_width,
                            pixel_height,
                        }) => {
                            let mut t = term.lock().unwrap();
                            if rows != t.screen_lines()
                                || cols != pty_cols
                            {
                                t.resize(TermSize::new(
                                    cols, rows,
                                ));
                                pty_cols = cols;
                                drop(t);
                                if pty_alive {
                                    let _ =
                                        master.resize(PtySize {
                                            rows: rows as u16,
                                            cols: cols as u16,
                                            pixel_width,
                                            pixel_height,
                                        });
                                }
                            }
                        }
                        Ok(TabEvent::Scroll(delta)) => {
                            term.lock()
                                .unwrap()
                                .scroll_display(Scroll::Delta(
                                    delta,
                                ));
                        }
                        Ok(TabEvent::ScrollTo(offset)) => {
                            let mut t = term.lock().unwrap();
                            let current =
                                t.grid().display_offset()
                                    as i32;
                            t.scroll_display(Scroll::Delta(
                                offset as i32 - current,
                            ));
                        }
                        Ok(TabEvent::SetSelection(sel)) => {
                            term.lock().unwrap().selection = sel;
                        }
                        Ok(TabEvent::UpdateSelection(
                            pt,
                            side,
                        )) => {
                            let mut t = term.lock().unwrap();
                            if let Some(sel) =
                                t.selection.as_mut()
                            {
                                sel.update(pt, side);
                            }
                        }
                        Ok(TabEvent::PtyEof) => {
                            pty_alive = false;
                            let exit_code = child
                                .wait()
                                .ok()
                                .map(|s| s.exit_code());
                            if let Some(code) = exit_code {
                                if code != 0 {
                                    let hint = format!(
                                        "\r\n[process exited \
                                         with code {}; {} + w \
                                         to close tab]\r\n",
                                        code, control_prefix,
                                    );
                                    let mut t =
                                        term.lock().unwrap();
                                    parser.advance(
                                        &mut *t,
                                        hint.as_bytes(),
                                    );
                                }
                            }
                            let _ =
                                futures::executor::block_on(
                                    sender.send(
                                        Message::ShellExited(
                                            id, exit_code,
                                        ),
                                    ),
                                );
                        }
                        Ok(TabEvent::Shutdown) | Err(_) => {
                            if pty_alive {
                                let _ = child.kill();
                                let _ = child.wait();
                            }
                            if let Some(wt) = &worktree_dir {
                                if let Some(dir) = &project_dir
                                {
                                    worktree::remove(dir, wt);
                                }
                            }
                            break;
                        }
                    }
                }

                let _ = exit_sender.send(());
            });

            let _ = exit_receiver.await;
        },
    )
}
