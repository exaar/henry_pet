use crate::{save_settings, settings::Settings, settings::SIZES, AppState};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

struct HideItem(CheckMenuItem<Wry>);
struct BedItem(CheckMenuItem<Wry>);
struct SkinItems(Vec<(&'static str, CheckMenuItem<Wry>)>);

fn check_skin(items: &[(&'static str, CheckMenuItem<Wry>)], id: &str) {
    for (sid, it) in items {
        let _ = it.set_checked(*sid == id);
    }
}

/// Keep the tray check marks in sync when the API hides/shows her or switches the skin.
pub fn sync(app: &AppHandle) {
    let st = app.state::<AppState>();
    let (hidden, skin, bed) = {
        let c = st.ctl.lock().unwrap();
        (c.hidden, c.skin.clone(), c.bed)
    };
    if let Some(item) = app.try_state::<BedItem>() {
        let _ = item.0.set_checked(bed);
    }
    if let Some(item) = app.try_state::<HideItem>() {
        let _ = item.0.set_checked(hidden);
    }
    if let Some(items) = app.try_state::<SkinItems>() {
        check_skin(&items.0, &skin);
    }
}

pub fn build(app: &AppHandle, s: &Settings) -> tauri::Result<()> {
    let hide = CheckMenuItem::with_id(app, "hide", "Hide Henry", true, s.hidden, None::<&str>)?;
    let sizes = SIZES
        .iter()
        .map(|&px| CheckMenuItem::with_id(app, format!("size{px}"), format!("{px} px"), true, s.size == px, None::<&str>))
        .collect::<tauri::Result<Vec<_>>>()?;
    let size_refs: Vec<&dyn tauri::menu::IsMenuItem<Wry>> = sizes.iter().map(|i| i as &dyn tauri::menu::IsMenuItem<Wry>).collect();
    let size = Submenu::with_id_and_items(app, "size", "Size", true, &size_refs)?;
    let current = crate::render::skin(&s.skin).id;
    let skins = crate::render::SKINS
        .iter()
        .map(|k| CheckMenuItem::with_id(app, format!("skin:{}", k.id), k.label, true, k.id == current, None::<&str>).map(|i| (k.id, i)))
        .collect::<tauri::Result<Vec<_>>>()?;
    let skin_refs: Vec<&dyn tauri::menu::IsMenuItem<Wry>> = skins.iter().map(|(_, i)| i as &dyn tauri::menu::IsMenuItem<Wry>).collect();
    let skin = Submenu::with_id_and_items(app, "skin", "Skin", true, &skin_refs)?;
    app.manage(SkinItems(skins.clone()));
    autostart::refresh();
    let auto = CheckMenuItem::with_id(app, "autostart", "Run at startup", true, autostart::enabled(), None::<&str>)?;
    let sound = CheckMenuItem::with_id(app, "sound", "Sound", true, s.sound, None::<&str>)?;
    let mischief = CheckMenuItem::with_id(app, "mischief", "Icon mischief", true, s.icon_mischief, None::<&str>)?;
    let bed = CheckMenuItem::with_id(app, "bed", "Bed", true, s.bed, None::<&str>)?;
    app.manage(BedItem(bed.clone()));
    let ret = MenuItem::with_id(app, "return", "Return icons", true, None::<&str>)?;
    let open = MenuItem::with_id(app, "settings", "Open settings file", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    app.manage(HideItem(hide.clone()));
    let sep2 = PredefinedMenuItem::separator(app)?;
    // "Skin" only when there is a choice (one sheet embedded unless HENRY_SPRITE_DIR / more skins come)
    let mut items: Vec<&dyn tauri::menu::IsMenuItem<Wry>> = vec![&hide, &size];
    if skins.len() > 1 {
        items.push(&skin);
    }
    items.extend([&sound as &dyn tauri::menu::IsMenuItem<Wry>, &bed, &mischief, &ret, &sep, &auto, &open, &sep2, &quit]);
    let menu = Menu::with_items(app, &items)?;

    let tray = TrayIconBuilder::with_id("henry")
        .icon(app.default_window_icon().cloned().expect("app icon"))
        // no tooltip: hovering the icon shows her status card instead (card.rs, PLAN #54)
        .menu(&menu)
        .on_tray_icon_event(|tray, ev| {
            let rect_of = |r: &tauri::Rect| {
                let p = r.position.to_physical::<f64>(1.0);
                let s = r.size.to_physical::<f64>(1.0);
                crate::win::Rect { left: p.x.round() as i32, top: p.y.round() as i32, right: (p.x + s.width).round() as i32, bottom: (p.y + s.height).round() as i32 }
            };
            let st = tray.app_handle().state::<AppState>();
            let mut c = st.ctl.lock().unwrap();
            match &ev {
                TrayIconEvent::Enter { rect, .. } | TrayIconEvent::Move { rect, .. } => {
                    c.tray.rect = Some(rect_of(rect));
                    c.tray.inside = true;
                }
                TrayIconEvent::Leave { .. } => {
                    c.tray.inside = false;
                    c.tray.suppressed = false;
                }
                TrayIconEvent::Click { rect, .. } | TrayIconEvent::DoubleClick { rect, .. } => {
                    c.tray.rect = Some(rect_of(rect));
                    c.tray.suppressed = true;
                }
                _ => {}
            }
        })
        .on_menu_event(move |app, ev| {
            let st = app.state::<AppState>();
            match ev.id().as_ref() {
                "hide" => {
                    let mut c = st.ctl.lock().unwrap();
                    c.hidden = !c.hidden;
                    let _ = hide.set_checked(c.hidden);
                    drop(c);
                }
                id if id.starts_with("skin:") => {
                    let id = &id[5..];
                    st.ctl.lock().unwrap().skin = id.to_string();
                    check_skin(&skins, id);
                }
                id if id.starts_with("size") => {
                    let n: u32 = id[4..].parse().unwrap_or(128);
                    st.ctl.lock().unwrap().size = n;
                    for (px, it) in SIZES.iter().zip(&sizes) {
                        let _ = it.set_checked(*px == n);
                    }
                }
                "sound" => {
                    let mut c = st.ctl.lock().unwrap();
                    c.sound = !c.sound;
                    let _ = sound.set_checked(c.sound);
                }
                "bed" => {
                    let mut c = st.ctl.lock().unwrap();
                    c.bed = !c.bed;
                    let _ = bed.set_checked(c.bed);
                }
                "mischief" => {
                    let mut c = st.ctl.lock().unwrap();
                    c.mischief = !c.mischief;
                    let _ = mischief.set_checked(c.mischief);
                }
                "return" => {
                    let path = st.ctl.lock().unwrap().icons_path.clone();
                    let r = crate::icons::return_all(&path);
                    st.ctl.lock().unwrap().note(format!("return icons (tray): {r:?}"));
                }
                "autostart" => {
                    let _ = autostart::set(!autostart::enabled());
                    let _ = auto.set_checked(autostart::enabled());
                }
                "settings" => {
                    let _ = std::process::Command::new("notepad").arg(&st.settings_path).spawn();
                }
                "quit" => app.exit(0),
                _ => {}
            }
            save_settings(&st);
        })
        .build(app)?;
    // where the icon is before any hover (status card placement for POST /card, tests)
    if let Ok(Some(r)) = tray.rect() {
        let p = r.position.to_physical::<f64>(1.0);
        let s = r.size.to_physical::<f64>(1.0);
        app.state::<AppState>().ctl.lock().unwrap().tray.rect =
            Some(crate::win::Rect { left: p.x.round() as i32, top: p.y.round() as i32, right: (p.x + s.width).round() as i32, bottom: (p.y + s.height).round() as i32 });
    }
    Ok(())
}

/// "Run at startup" = HKCU Run value; no admin, no watchdog (PLAN #18).
pub mod autostart {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    const KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const NAME: &str = "henry_pet";

    /// The exe moved (e.g. installed elsewhere): point the startup entry at this one.
    pub fn refresh() {
        if enabled() {
            let _ = set(true);
        }
    }

    pub fn enabled() -> bool {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(KEY)
            .and_then(|k| k.get_value::<String, _>(NAME))
            .is_ok()
    }

    pub fn set(on: bool) -> std::io::Result<()> {
        let (k, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(KEY)?;
        if on {
            let exe = std::env::current_exe()?;
            k.set_value(NAME, &format!("\"{}\"", exe.display()))
        } else {
            k.delete_value(NAME)
        }
    }
}
