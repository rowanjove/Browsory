use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};

pub fn setup_tray(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let open_item = MenuItem::with_id(app, "open", "打开 Browsory", true, None::<&str>)?;
    let search_item =
        MenuItem::with_id(app, "quick_search", "快速搜索 (Ctrl+K)", true, None::<&str>)?;
    let sync_item = MenuItem::with_id(app, "sync_now", "立即同步数据源", true, None::<&str>)?;
    let lock_item = MenuItem::with_id(app, "lock", "锁定应用 (Ctrl+L)", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "退出 Browsory", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[&open_item, &search_item, &sync_item, &lock_item, &quit_item],
    )?;

    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("Browsory - 本地优先的网络记忆库")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quick_search" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                    let _ = window.emit("browsory://open-quick-search", ());
                }
            }
            "sync_now" => {
                let app_handle = app.clone();
                std::thread::spawn(move || {
                    if let Some(window) = app_handle.get_webview_window("main") {
                        let _ = window.emit("browsory://sync-now", ());
                    }
                });
            }
            "lock" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.emit("browsory://lock", ());
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app)?;
    tracing::info!("System tray initialized successfully.");
    Ok(())
}
