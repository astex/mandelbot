mod config;
mod keys;
mod mcp;
mod pty;
mod terminal;
mod theme;
mod ui;
mod widget;

fn main() -> iced::Result {
    let args: Vec<String> = std::env::args().collect();

    if args.contains(&"--mcp-server".to_string()) {
        let session_id = arg_value(&args, "--session-id").expect("--session-id required");
        let parent_socket =
            arg_value(&args, "--parent-socket").expect("--parent-socket required");

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");

        if let Err(e) = rt.block_on(mcp::run(
            &session_id,
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

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}
