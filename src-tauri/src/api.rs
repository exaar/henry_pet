//! Agent API: local HTTP on 127.0.0.1 only (PLAN #19).
use crate::brain::{BedCmd, Command, Control, Mood};
use serde::Deserialize;
use serde_json::json;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tiny_http::{Header, Method, Response, Server};

#[derive(Deserialize)]
struct MoodReq {
    state: Mood,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    ttl_s: Option<f64>,
}

#[derive(Deserialize)]
struct SkinReq {
    skin: String,
}

#[derive(Deserialize)]
struct BedReq {
    action: String,
    #[serde(default)]
    x: Option<f64>,
}

#[derive(Deserialize)]
struct BondReq {
    bond: f64,
}

#[derive(Deserialize)]
struct NeedsReq {
    #[serde(default)]
    sleepiness: Option<f64>,
    #[serde(default)]
    boredom: Option<f64>,
    #[serde(default)]
    playfulness: Option<f64>,
}

#[derive(Deserialize)]
struct CardReq {
    #[serde(default)]
    show: Option<bool>,
    #[serde(default)]
    pin: Option<bool>,
    #[serde(default)]
    x: Option<i32>,
    #[serde(default)]
    y: Option<i32>,
}

#[derive(Deserialize)]
struct DoReq {
    action: Command,
}

const INDEX: &str = r#"{
  "name": "henry_pet",
  "endpoints": {
    "GET /status": "what she is doing, needs, mood, position",
    "GET /hermes-plugin": "where the bundled Hermes bridge plugin lives (dir) and its agent install guide (guide = INSTALL.md)",
    "POST /mood": "{\"state\": \"idle|review|run|waiting|wave|jump|failed\", \"source\": \"hermes:<session>\", \"ttl_s\": 600} - lasting moods expire after ttl_s; wave/jump/failed are one-shot",
    "POST /do": "{\"action\": \"sleep|wake|walk|come|wave|zoomies|idle|groom|stretch|jump|climb|down|carry|meow|greet|bed|movebed|warm|glow\"} - greet: come and meow as after a long absence (if calm), bed: go and lie in her bed, movebed: drag it somewhere else, warm: nap on the window of the busiest GPU process now, glow: show the petting halo for 3 s (nothing else changes)",
    "GET /bed": "her bed: mode (floor|window|air|held|carried), position, squash, on",
    "POST /bed": "{\"action\": \"show|hide|place\", \"x\": 1200} - place drops it at screen x (default: next to her) onto whatever is below",
    "GET /icons": "desktop icons she moved and where they came from",
    "POST /icons/return": "put every moved icon back",
    "GET /skins": "embedded sprite sets and the current one",
    "POST /skin": "{\"skin\": \"henry\"} - switch the sprite set live (saved like the tray choice)",
    "POST /bond": "{\"bond\": 0.42} - set her bond with the user (0..1; levels awful|bad|neutral|good|amazing|owner, shown in /status.bond)",
    "POST /needs": "{\"sleepiness\": 0.5, \"boredom\": 0.1, \"playfulness\": 0.9} - set her needs (0..1, any subset)",
    "POST /card": "{\"show\": true} - show her status card (the tray hover card) without hovering; false hands it back to the hover. {\"pin\": true|false} pins / unpins it (pinned = stays open, like the pushpin on the card), {\"x\": 100, \"y\": 100} moves it there and pins it; /status.card has pinned + rect",
    "POST /hide": "hide Henry",
    "POST /show": "show Henry"
  }
}"#;

/// `on_change` runs after the API changed a tray-visible setting (hide/show, skin): sync tray + save.
pub fn spawn(port: u16, ctl: Arc<Mutex<Control>>, on_change: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        let server = match Server::http(("127.0.0.1", port)) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("henry_pet: API not started on 127.0.0.1:{port}: {e}");
                return;
            }
        };
        let ct = Header::from_bytes("Content-Type", "application/json").unwrap();
        for mut req in server.incoming_requests() {
            let mut body = String::new();
            let _ = req.as_reader().take(64 * 1024).read_to_string(&mut body);
            let (code, out) = handle(req.method(), req.url(), &body, &ctl, &on_change);
            let _ = req.respond(Response::from_string(out).with_status_code(code).with_header(ct.clone()));
        }
    });
}

fn handle(m: &Method, url: &str, body: &str, ctl: &Arc<Mutex<Control>>, on_change: &impl Fn()) -> (u16, String) {
    let path = url.split('?').next().unwrap_or("");
    let err = |code: u16, msg: String| (code, json!({"ok": false, "error": msg}).to_string());
    match (m, path) {
        (Method::Get, "/") => (200, INDEX.to_string()),
        (Method::Get, "/status") => (200, ctl.lock().unwrap().status.to_string()),
        (Method::Get, "/hermes-plugin") => {
            // installed layout: <install dir>/hermes-plugin/{INSTALL.md, henry_pet/}; dev builds: the repo copy
            let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf()));
            let repo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("hermes-plugin");
            let base = exe_dir.map(|d| d.join("hermes-plugin")).filter(|d| d.join("henry_pet").is_dir()).unwrap_or(repo);
            let found = base.join("henry_pet").is_dir();
            (200, json!({"ok": found, "dir": base.join("henry_pet").display().to_string(), "guide": base.join("INSTALL.md").display().to_string(), "version": env!("CARGO_PKG_VERSION")}).to_string())
        }
        (Method::Post, "/mood") => match serde_json::from_str::<MoodReq>(body) {
            Ok(r) => {
                let ttl = Duration::from_secs_f64(r.ttl_s.unwrap_or(600.0).clamp(1.0, 3600.0));
                ctl.lock().unwrap().set_mood(r.source.unwrap_or_else(|| "api".into()), r.state, ttl);
                (200, json!({"ok": true}).to_string())
            }
            Err(e) => err(400, e.to_string()),
        },
        (Method::Post, "/do") => match serde_json::from_str::<DoReq>(body) {
            Ok(r) => {
                ctl.lock().unwrap().commands.push_back(r.action);
                (200, json!({"ok": true}).to_string())
            }
            Err(e) => err(400, e.to_string()),
        },
        (Method::Post, "/bond") => match serde_json::from_str::<BondReq>(body) {
            Ok(r) if (0.0..=1.0).contains(&r.bond) => {
                ctl.lock().unwrap().bond_set = Some(r.bond);
                (200, json!({"ok": true}).to_string())
            }
            Ok(_) => err(400, "bond must be 0..1".into()),
            Err(e) => err(400, e.to_string()),
        },
        (Method::Post, "/needs") => match serde_json::from_str::<NeedsReq>(body) {
            Ok(r) => {
                ctl.lock().unwrap().needs_set = Some([r.sleepiness, r.boredom, r.playfulness]);
                (200, json!({"ok": true}).to_string())
            }
            Err(e) => err(400, e.to_string()),
        },
        (Method::Post, "/card") => match serde_json::from_str::<CardReq>(body) {
            Ok(r) => {
                let mut c = ctl.lock().unwrap();
                if let Some(s) = r.show {
                    c.tray.forced = s;
                }
                let pos = r.x.zip(r.y);
                if r.pin.is_some() || pos.is_some() {
                    c.card_pin = Some((r.pin, pos));
                }
                (200, json!({"ok": true, "show": c.tray.forced, "pin": r.pin, "pos": pos}).to_string())
            }
            Err(e) => err(400, e.to_string()),
        },
        (Method::Get, "/bed") => (200, ctl.lock().unwrap().status.get("bed").cloned().unwrap_or(json!(null)).to_string()),
        (Method::Post, "/bed") => match serde_json::from_str::<BedReq>(body) {
            Ok(r) => match r.action.as_str() {
                "show" | "hide" => {
                    ctl.lock().unwrap().bed = r.action == "show";
                    on_change();
                    (200, json!({"ok": true, "on": r.action == "show"}).to_string())
                }
                "place" => {
                    let mut c = ctl.lock().unwrap();
                    c.bed = true;
                    c.bed_cmds.push_back(BedCmd::Place(r.x));
                    drop(c);
                    on_change();
                    (200, json!({"ok": true}).to_string())
                }
                a => err(400, format!("unknown bed action '{a}' (show|hide|place)")),
            },
            Err(e) => err(400, e.to_string()),
        },
        (Method::Get, "/icons") => {
            let path = ctl.lock().unwrap().icons_path.clone();
            let s = crate::icons::load(&path);
            let lv = crate::icons::desktop_listview();
            let desktop = lv.and_then(crate::icons::list).map(|v| {
                v.iter().map(|i| json!({"name": i.name, "rect": [i.rect.left, i.rect.top, i.rect.right, i.rect.bottom]})).collect::<Vec<_>>()
            });
            let blocked = lv.map_or(Some("no desktop list view"), crate::icons::blocked);
            (200, json!({"moved": s.moved, "hasSnapshot": s.snapshot.is_some(), "blocked": blocked, "desktop": desktop}).to_string())
        }
        (Method::Post, "/icons/return") => {
            let path = ctl.lock().unwrap().icons_path.clone();
            match crate::icons::return_all(&path) {
                Ok(n) => (200, json!({"ok": true, "returned": n}).to_string()),
                Err(e) => err(500, e),
            }
        }
        (Method::Post, "/hide") | (Method::Post, "/show") => {
            let hidden = path == "/hide";
            ctl.lock().unwrap().hidden = hidden;
            on_change();
            (200, json!({"ok": true, "hidden": hidden}).to_string())
        }
        (Method::Get, "/skins") => {
            let skins: Vec<_> = crate::render::SKINS.iter().map(|s| json!({"id": s.id, "label": s.label})).collect();
            (200, json!({"current": ctl.lock().unwrap().skin, "skins": skins}).to_string())
        }
        (Method::Post, "/skin") => match serde_json::from_str::<SkinReq>(body) {
            Ok(r) if crate::render::SKINS.iter().any(|s| s.id == r.skin) => {
                ctl.lock().unwrap().skin = r.skin.clone();
                on_change();
                (200, json!({"ok": true, "skin": r.skin}).to_string())
            }
            Ok(r) => err(400, format!("unknown skin '{}'", r.skin)),
            Err(e) => err(400, e.to_string()),
        },
        _ => err(404, format!("no route {path}")),
    }
}
