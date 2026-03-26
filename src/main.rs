mod config;
mod keys;
mod links;
mod mcp;
mod pty;
mod terminal;
mod theme;
mod ui;
mod widget;

fn main() -> iced::Result {
    let args: Vec<String> = std::env::args().collect();

    if let Some(pos) = args.iter().position(|a| a == "--set-status") {
        let status = args.get(pos + 1).expect("--set-status requires a status value");
        let tab_id = std::env::var("MANDELBOT_TAB_ID").unwrap_or_default();
        let parent_socket = std::env::var("MANDELBOT_PARENT_SOCKET").unwrap_or_default();

        if tab_id.is_empty() || parent_socket.is_empty() {
            return Ok(());
        }

        let msg = serde_json::json!({
            "type": "set_status",
            "tab_id": tab_id,
            "status": status,
        });
        let mut msg_str = serde_json::to_string(&msg).unwrap();
        msg_str.push('\n');

        use std::io::Write;
        if let Ok(mut stream) = std::os::unix::net::UnixStream::connect(&parent_socket) {
            let _ = stream.write_all(msg_str.as_bytes());
            let _ = stream.flush();
        }
        return Ok(());
    }

    if args.contains(&"--mcp-server".to_string()) {
        let tab_id =
            std::env::var("MANDELBOT_TAB_ID").expect("MANDELBOT_TAB_ID required");
        let parent_socket =
            std::env::var("MANDELBOT_PARENT_SOCKET").expect("MANDELBOT_PARENT_SOCKET required");

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");

        if let Err(e) = rt.block_on(mcp::run(
            &tab_id,
            std::path::Path::new(&parent_socket),
        )) {
            eprintln!("mcp server error: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }

    let cfg = config::Config::load();
    let window_size = ui::initial_window_size(&cfg);

    iced::application(ui::App::boot, ui::App::update, ui::App::view)
        .title("Mandelbot")
        .subscription(ui::App::subscription)
        .theme(ui::App::theme)
        .window_size(window_size)
        .run()
}
