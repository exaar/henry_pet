//! Record / replay of everything the brain reads from the outside world (refactor safety net).
//!
//! Every input of a tick - the clock, cursor and button, monitors, windows, the foreground app, idle time,
//! GPU load, desktop icons, API requests (Control) - goes through the tape. Live, the value is taken from
//! the system (and written to `tape.txt` when recording). In a replay it is read back from the file in
//! the same order, so the brain runs exactly the ticks it ran live, as fast as it can, without showing a
//! window, and writes one trace line per tick (`--trace`). Same tape + same code = same trace, byte for
//! byte: a refactor that changes the trace changed behaviour.
//!
//! Scenarios: `overrides.jsonl` next to the tape replaces inputs over tick ranges (a grab, petting, a hot
//! GPU, an API command...). Once a scenario changes what she does, reads that only happen in some
//! situations (`OPTIONAL`) no longer line up with the tape: the replay skips recorded ones nobody asks
//! for and answers unrecorded ones with the caller's default - the same way every run, so scenario
//! traces are as repeatable as plain ones.
//!
//! Record: `HENRY_RECORD=<dir>` when starting Henry (the dir gets tape.txt + copies of her state files,
//! and live-trace.txt, which a replay of the tape must reproduce). Replay: `henry_pet.exe --replay <dir>
//! --trace <file>`.
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// What a recording starts from (first line of tape.txt).
#[derive(Serialize, Deserialize, Clone)]
pub struct Header {
    pub seed: u64,
    pub time_scale: f64,
    pub start_x: Option<String>,
    pub skin: String,
    /// bond after load (load drifts it by wall-clock days, which differ at replay time)
    pub bond: f64,
}

/// State files of hers the brain reads at start; copied into the recording and read from there in a replay.
pub const STATE_FILES: [&str; 3] = ["bed.json", "bond.json", "card.json"];

/// Reads made only in some situations (see the module doc).
const OPTIONAL: [&str; 8] = ["frame", "icons", "setpos", "exe", "grabinfo", "movesize", "warm_cmd", "meow"];

#[derive(Deserialize)]
struct Override {
    from: u64,
    to: u64,
    tag: String,
    /// replace the whole value / one element of an array value (`index`)
    #[serde(default)]
    value: Option<Value>,
    #[serde(default)]
    index: Option<usize>,
    /// cursor relative to an anchor the brain sets each tick ("henry", "bed"): anchor + path[(tick-from) % len]
    #[serde(default)]
    rel: Option<String>,
    #[serde(default)]
    path: Vec<(f64, f64)>,
}

struct Replay {
    lines: Vec<String>,
    pos: usize,
    last: HashMap<String, String>,
    overrides: Vec<Override>,
    anchors: HashMap<&'static str, (f64, f64)>,
}

enum Mode {
    Live { out: Option<BufWriter<std::fs::File>>, last: HashMap<&'static str, String> },
    Replay(Box<Replay>),
}

pub struct Tape {
    mode: Mode,
    pub t0: Instant,
    pub tick: u64,
    pub header: Option<Header>,
}

impl Tape {
    /// Live; recording if HENRY_RECORD is set (state files are copied from `state_dir`).
    pub fn live(state_dir: &Path) -> Tape {
        let out = std::env::var("HENRY_RECORD").ok().and_then(|d| {
            let dir = PathBuf::from(d);
            std::fs::create_dir_all(&dir).ok()?;
            for f in STATE_FILES {
                let _ = std::fs::copy(state_dir.join(f), dir.join(f));
            }
            std::fs::File::create(dir.join("tape.txt")).ok().map(BufWriter::new)
        });
        Tape { mode: Mode::Live { out, last: HashMap::new() }, t0: Instant::now(), tick: 0, header: None }
    }

    pub fn replay(dir: &Path) -> Tape {
        let text = std::fs::read_to_string(dir.join("tape.txt")).expect("tape.txt");
        let mut lines: Vec<String> = text.lines().map(String::from).collect();
        let first = lines.remove(0);
        let header: Header = serde_json::from_str(first.strip_prefix("header\t").expect("tape header")).expect("tape header json");
        let overrides = std::fs::read_to_string(dir.join("overrides.jsonl"))
            .map(|t| {
                t.lines()
                    .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with("//"))
                    .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("overrides.jsonl: {e}: {l}")))
                    .collect()
            })
            .unwrap_or_default();
        let r = Replay { lines, pos: 0, last: HashMap::new(), overrides, anchors: HashMap::new() };
        Tape { mode: Mode::Replay(Box::new(r)), t0: Instant::now(), tick: 0, header: Some(header) }
    }

    pub fn replaying(&self) -> bool {
        matches!(self.mode, Mode::Replay(_))
    }

    /// Replay scenarios: where things are this tick (cursor overrides can be relative to them).
    pub fn anchor(&mut self, name: &'static str, x: f64, y: f64) {
        if let Mode::Replay(r) = &mut self.mode {
            r.anchors.insert(name, (x, y));
        }
    }

    /// Live: write the header (what the replay must start from). Replay: the recorded one.
    pub fn header(&mut self, live: impl FnOnce() -> Header) -> Header {
        if let Some(h) = &self.header {
            return h.clone();
        }
        let h = live();
        if let Mode::Live { out: Some(o), .. } = &mut self.mode {
            let _ = writeln!(o, "header\t{}", serde_json::to_string(&h).unwrap());
        }
        self.header = Some(h.clone());
        h
    }

    /// Start of a tick: live sleeps a frame and reads the clock; a replay takes the recorded time
    /// (None when the tape is over).
    pub fn next_tick(&mut self) -> Option<Instant> {
        let t0 = self.t0;
        let live = matches!(self.mode, Mode::Live { .. });
        if live {
            pace_to_vsync();
        }
        self.tick += 1;
        let ns: Option<u64> = self.get("tick", None, None::<fn() -> u64>, || (Instant::now() - t0).as_nanos() as u64);
        if let Mode::Live { out: Some(o), .. } = &mut self.mode {
            let _ = o.flush();
        }
        ns.map(|ns| t0 + Duration::from_nanos(ns))
    }

    /// An input: live from `f` (recorded), in a replay the next recorded value (must carry the same tag).
    pub fn val<T: Serialize + DeserializeOwned>(&mut self, tag: &'static str, f: impl FnOnce() -> T) -> T {
        let tick = self.tick;
        self.get(tag, None, None::<fn() -> T>, f).unwrap_or_else(|| panic!("tape over in the middle of tick {tick} (wanted {tag})"))
    }

    /// An OPTIONAL input with the answer a scenario replay gives when the tape has none here.
    pub fn val_or<T: Serialize + DeserializeOwned>(&mut self, tag: &'static str, default: impl FnOnce() -> T, f: impl FnOnce() -> T) -> T {
        let tick = self.tick;
        self.get(tag, None, Some(default), f).unwrap_or_else(|| panic!("tape over in the middle of tick {tick} (wanted {tag})"))
    }

    /// Keyed OPTIONAL input (a window's frame, a pid's exe): recorded as [key, value]; a recorded value
    /// for another key is not this read.
    pub fn keyed<K, T>(&mut self, tag: &'static str, key: K, default: impl FnOnce() -> T, f: impl FnOnce() -> T) -> T
    where
        K: Serialize + DeserializeOwned + Clone,
        T: Serialize + DeserializeOwned,
    {
        let tick = self.tick;
        let kv = serde_json::to_value(&key).unwrap();
        let k2 = key.clone();
        let (_, v): (K, T) = self
            .get(tag, Some(kv), Some(|| (k2, default())), || (key, f()))
            .unwrap_or_else(|| panic!("tape over in the middle of tick {tick} (wanted {tag})"));
        v
    }

    fn get<T, D>(&mut self, tag: &'static str, key: Option<Value>, default: Option<D>, f: impl FnOnce() -> T) -> Option<T>
    where
        T: Serialize + DeserializeOwned,
        D: FnOnce() -> T,
    {
        let tick = self.tick;
        match &mut self.mode {
            Mode::Live { out, last } => {
                let v = f();
                if let Some(o) = out {
                    let s = serde_json::to_string(&v).unwrap();
                    if last.get(tag) == Some(&s) {
                        let _ = writeln!(o, "{tag}\t=");
                    } else {
                        let _ = writeln!(o, "{tag}\t{s}");
                        last.insert(tag, s);
                    }
                }
                Some(v)
            }
            Mode::Replay(r) => {
                let optional = OPTIONAL.contains(&tag);
                let scenario = !r.overrides.is_empty();
                // the recorded value, if the tape has one for this read here
                let mut found: Option<Value> = None;
                while let Some(line) = r.lines.get(r.pos) {
                    let (t, s) = line.split_once('\t').unwrap_or_else(|| panic!("tape line {}: no tab", r.pos + 2));
                    let s = if s == "=" { r.last.get(t).cloned().expect("'=' with nothing before") } else { s.to_string() };
                    let t = t.to_string();
                    let opt_line = OPTIONAL.contains(&t.as_str());
                    if t == tag {
                        let v: Value = serde_json::from_str(&s).unwrap_or_else(|e| panic!("tape line {} ({tag}): {e}", r.pos + 2));
                        let same_key = key.as_ref().map_or(true, |k| v.get(0) == Some(k));
                        if same_key || !scenario {
                            r.pos += 1;
                            r.last.insert(t, s);
                            found = Some(v);
                            break;
                        }
                    }
                    if opt_line && scenario {
                        r.pos += 1; // recorded, but this (diverged) run doesn't make that read
                        r.last.insert(t, s);
                        continue;
                    }
                    if optional && scenario {
                        break; // not recorded here: the default answers
                    }
                    panic!("tape desync at line {} (tick {tick}): code asks for '{tag}', tape has '{t}'", r.pos + 2);
                }
                let mut v: Value = match found {
                    Some(v) => v,
                    None if r.pos >= r.lines.len() && !(optional && scenario) => return None,
                    None => match default {
                        Some(d) => serde_json::to_value(d()).unwrap(),
                        None => panic!("tape has no '{tag}' at tick {tick}"),
                    },
                };
                for o in r.overrides.iter().filter(|o| o.tag == tag && o.from <= tick && tick <= o.to) {
                    if let Some(name) = &o.rel {
                        let (ax, ay) = r.anchors.get(name.as_str()).copied().unwrap_or((0.0, 0.0));
                        let (dx, dy) = o.path[((tick - o.from) as usize) % o.path.len().max(1)];
                        v = serde_json::json!([(ax + dx).round() as i64, (ay + dy).round() as i64]);
                    } else if let Some(val) = &o.value {
                        match o.index {
                            Some(i) => v[i] = val.clone(),
                            None => v = val.clone(),
                        }
                    }
                }
                Some(serde_json::from_value(v).unwrap_or_else(|e| panic!("tape value for {tag} at tick {tick}: {e}")))
            }
        }
    }
}

/// Live pacing: one brain tick per screen refresh. DwmFlush returns right after the next desktop
/// composition (vsync), so every window move and frame change lands on its own refresh; the old fixed
/// 16 ms sleep beat against the 60 Hz screen and drifted (now and then two ticks in one refresh or none).
/// If DWM can't pace us (composition paused: locked, screen off) it returns at once - then sleep to ~60 Hz.
fn pace_to_vsync() {
    use std::cell::Cell;
    thread_local! { static LAST: Cell<Option<Instant>> = const { Cell::new(None) }; }
    let ok = unsafe { windows::Win32::Graphics::Dwm::DwmFlush() }.is_ok();
    let now = Instant::now();
    let since = LAST.with(|l| l.get()).map(|t| now - t);
    if !ok || since.is_some_and(|d| d < Duration::from_millis(4)) {
        std::thread::sleep(Duration::from_millis(16).saturating_sub(since.unwrap_or_default()));
    }
    LAST.with(|l| l.set(Some(Instant::now())));
}
