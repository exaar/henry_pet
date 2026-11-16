#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod bed;
mod bond;
mod brain;
mod card;
mod gpu;
mod halo;
mod icons;
mod memfuse;
mod render;
mod settings;
mod slinky;
mod sound;
mod tape;
mod tray;
mod win;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tauri::Manager;

pub struct AppState {
    pub ctl: Arc<Mutex<brain::Control>>,
    pub settings_path: PathBuf,
    pub api_port: u16,
}

pub fn save_settings(state: &AppState) {
    let c = state.ctl.lock().unwrap();
    settings::save(
        &state.settings_path,
        &settings::Settings { size: c.size, scale: None, hidden: c.hidden, quiet_apps: c.quiet_apps.clone(), api_port: state.api_port, sound: c.sound, icon_mischief: c.mischief, skin: c.skin.clone(), bed: c.bed },
    );
}

/// Hand edits to settings.json (e.g. the quiet-apps list) apply without a restart.
fn watch_settings(path: PathBuf, ctl: Arc<Mutex<brain::Control>>) {
    std::thread::spawn(move || {
        let mtime = |p: &PathBuf| std::fs::metadata(p).and_then(|m| m.modified()).ok();
        let mut seen: Option<SystemTime> = mtime(&path);
        loop {
            std::thread::sleep(Duration::from_secs(2));
            let now = mtime(&path);
            if now != seen {
                seen = now;
                let s = settings::load(&path);
                let mut c = ctl.lock().unwrap();
                c.quiet_apps = s.quiet_apps;
                c.size = s.size;
                c.sound = s.sound;
                c.mischief = s.icon_mischief;
                c.skin = s.skin;
                c.bed = s.bed;
            }
        }
    });
}

fn main() {
    // refactor safety net (tape.rs): replay a recording headless, write the per-tick trace, exit
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--replay") {
        let dir = PathBuf::from(&args[i + 1]);
        let out = args.iter().position(|a| a == "--trace").map(|j| PathBuf::from(&args[j + 1])).unwrap_or_else(|| dir.join("trace.txt"));
        brain::replay(&dir, &out);
        return;
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|_app, _args, _cwd| {}))
        .setup(|app| {
            let dir = app.path().app_config_dir()?;
            std::fs::create_dir_all(&dir)?;
            let path = dir.join("settings.json");
            let first_run = !path.exists();
            let mut s = settings::load(&path);
            if first_run {
                // fresh install: size to the screen and come back after a reboot (both changeable in the tray)
                s.size = settings::size_for_screen();
                settings::save(&path, &s);
                let _ = tray::autostart::set(true);
            }
            let mut control = brain::Control::new(s.hidden, s.size, s.quiet_apps.clone());
            control.sound = s.sound;
            control.mischief = s.icon_mischief;
            control.bed = s.bed;
            control.icons_path = dir.join("icons.json");
            control.skin = crate::render::skin(&s.skin).id.to_string();
            let ctl = Arc::new(Mutex::new(control));

            app.manage(AppState { ctl: ctl.clone(), settings_path: path.clone(), api_port: s.api_port });
            tray::build(app.handle(), &s)?;
            let handle = app.handle().clone();
            api::spawn(s.api_port, ctl.clone(), move || {
                tray::sync(&handle);
                save_settings(&handle.state::<AppState>());
            });
            watch_settings(path, ctl.clone());
            memfuse::spawn(dir.clone(), ctl.clone());
            brain::spawn(ctl);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running henry_pet");
}
