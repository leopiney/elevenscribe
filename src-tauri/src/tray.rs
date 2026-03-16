use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;

pub fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let quit = MenuItemBuilder::with_id("quit", "Quit Elevenscribe").build(app)?;
    let menu = MenuBuilder::new(app).items(&[&quit]).build()?;

    TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .on_menu_event(|app, event| {
            if event.id.as_ref() == "quit" {
                app.exit(0);
            }
        })
        .build(app)?;

    Ok(())
}
