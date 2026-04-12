mod animation;
mod config;
mod effect;
mod headless;
mod host;
mod keys;
mod links;
mod mcp;
mod pty;
mod tab;
mod theme;
mod worktree;
mod ui;
mod widget;

fn main() -> iced::Result {
    let args: Vec<String> = std::env::args().collect();

    if let Some(i) = args.iter().position(|a| a == "--headless") {
        let Some(scenario_path) = args.get(i + 1) else {
            eprintln!("error: --headless requires a scenario path argument");
            eprintln!("usage: mandelbot --headless <scenario.json>");
            std::process::exit(2);
        };
        if let Err(e) = headless::run(std::path::Path::new(scenario_path)) {
            eprintln!("headless error: {e}");
            std::process::exit(1);
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

    let icon = iced::window::icon::from_rgba(
        include_bytes!("../assets/icons/logo-32x32.rgba").to_vec(),
        32,
        32,
    )
    .ok();

    let mut app = iced::application(host::IcedHost::boot, host::IcedHost::update, host::IcedHost::view)
        .title("Mandelbot")
        .subscription(host::IcedHost::subscription)
        .theme(host::IcedHost::theme)
        .window(iced::window::Settings {
            size: window_size,
            icon,
            ..Default::default()
        });

    for font_bytes in config::find_font_variants(&cfg.font) {
        app = app.font(font_bytes);
    }

    app.run()
}
