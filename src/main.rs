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
