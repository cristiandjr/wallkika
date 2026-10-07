use crate::{
    commands, show_about, show_panel,
    updates::{UpdateInfo, Updates},
};
use tauri::{
    image::Image,
    menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager, Wry,
};

const TRAY_ID: &str = "wallkika";

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(include_bytes!("../icons/tray.png"))?)
        .tooltip("WallKika")
        .menu(&build_menu(app, None)?)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "update" => {
                if let Err(err) = commands::open_update(app.clone()) {
                    log::warn!("Could not open the release page: {err}");
                }
            }
            "panel" => show_panel(app),
            "stop" => {
                tauri::async_runtime::spawn(commands::clear_wallpaper(app.clone(), None));
            }
            "about" => show_about(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

pub fn refresh(app: &AppHandle) -> tauri::Result<()> {
    let update = app.state::<Updates>().get();
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_menu(Some(build_menu(app, update.as_ref())?))?;
    }
    Ok(())
}

fn build_menu(app: &AppHandle, update: Option<&UpdateInfo>) -> tauri::Result<Menu<Wry>> {
    let item = |id: &str, text: &str| MenuItem::with_id(app, id, text, true, None::<&str>);
    let mut items: Vec<Box<dyn IsMenuItem<Wry>>> = Vec::new();
    if let Some(update) = update {
        let text = format!("New version {} available…", update.version);
        items.push(Box::new(item("update", &text)?));
        items.push(Box::new(PredefinedMenuItem::separator(app)?));
    }
    items.push(Box::new(item("panel", "Open WallKika")?));
    items.push(Box::new(item("stop", "Stop live wallpapers")?));
    items.push(Box::new(PredefinedMenuItem::separator(app)?));
    items.push(Box::new(item("about", "About WallKika")?));
    items.push(Box::new(PredefinedMenuItem::separator(app)?));
    items.push(Box::new(item("quit", "Quit WallKika")?));
    let refs: Vec<&dyn IsMenuItem<Wry>> = items.iter().map(|item| item.as_ref()).collect();
    Menu::with_items(app, &refs)
}
