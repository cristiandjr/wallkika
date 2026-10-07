use crate::{commands, report_error, show_panel};
use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle,
};

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let choose = MenuItem::with_id(app, "choose", "Choose wallpaper…", true, None::<&str>)?;
    let panel = MenuItem::with_id(app, "panel", "Open WallKika", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop", "Stop live wallpaper", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit WallKika", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&choose, &panel, &stop, &separator, &quit])?;

    TrayIconBuilder::with_id("wallkika")
        .icon(Image::from_bytes(include_bytes!("../icons/tray.png"))?)
        .tooltip("WallKika")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "choose" => {
                show_panel(app);
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(err) = commands::choose_wallpaper(app.clone()).await {
                        report_error(&app, &err);
                    }
                });
            }
            "panel" => show_panel(app),
            "stop" => {
                tauri::async_runtime::spawn(commands::stop_live_wallpaper(app.clone()));
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}
