use serde::{Deserialize, Serialize};
use std::path::Path;

/// Tray choices for her on-screen height (px of one sprite cell).
pub const SIZES: [u32; 6] = [64, 96, 128, 160, 192, 256];

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    /// On-screen height of one sprite cell in px, independent of the source art (PLAN #17); 32..=512.
    pub size: u32,
    /// Old 1x/2x/3x multiplier (64 px * scale); read once to migrate, never written.
    #[serde(skip_serializing)]
    pub scale: Option<u32>,
    pub hidden: bool,
    /// Executable names where Henry sits still in a corner (PLAN #11).
    pub quiet_apps: Vec<String>,
    /// Agent API port on 127.0.0.1 (PLAN #19).
    pub api_port: u16,
    /// Meow / purr (PLAN #12): off by default.
    pub sound: bool,
    /// Desktop icon mischief (PLAN #10): off by default (public build); turn it on in the tray.
    pub icon_mischief: bool,
    /// Sprite set id (tray "Skin"): "henry"; unknown (e.g. the retired "placeholder") -> the first embedded one.
    pub skin: String,
    /// Her bed is out (tray "Bed", PLAN #51).
    pub bed: bool,
    /// Daily check for a newer public release (update.rs, PLAN #65); hand edit only.
    pub update_check: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { size: 128, scale: None, hidden: false, quiet_apps: vec!["Resolve.exe".into()], api_port: 47821, sound: false, icon_mischief: false, skin: "henry".into(), bed: true, update_check: true }
    }
}

pub fn load(path: &Path) -> Settings {
    let text = std::fs::read_to_string(path).ok();
    let mut s = text.as_deref().and_then(|t| serde_json::from_str::<Settings>(t).ok()).unwrap_or_else(|| {
        let d = Settings::default();
        if !path.exists() {
            save(path, &d);
        }
        d
    });
    let has_size = text.as_deref().and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok()).is_some_and(|v| v.get("size").is_some());
    if let (false, Some(k)) = (has_size, s.scale) {
        s.size = 64 * k.clamp(1, 4);
    }
    s.scale = None;
    s.size = s.size.clamp(32, 512);
    s
}

/// First-run size: what looks right on a 1440p screen (192 px) scaled by the primary monitor's height
/// (physical px, so 4K / high-DPI screens get a bigger cat), snapped to the tray sizes.
pub fn size_for_screen() -> u32 {
    let h = crate::win::monitors().iter().find(|m| m.primary).map(|m| m.rect.bottom - m.rect.top).unwrap_or(1080).max(480);
    let want = 192.0 * h as f64 / 1440.0;
    *SIZES.iter().min_by(|a, b| (**a as f64 - want).abs().total_cmp(&(**b as f64 - want).abs())).unwrap()
}

pub fn save(path: &Path, s: &Settings) {
    if let Ok(t) = serde_json::to_string_pretty(s) {
        let _ = std::fs::write(path, t);
    }
}
