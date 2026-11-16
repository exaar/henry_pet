//! Henry's body and mind. Rust owns where she is and what she does; the webview only draws the row it is told.
//!
//! Needs (0..1) grow over time and pick behaviour: sleepiness (faster at night and while the user is away),
//! boredom (comes to the cursor to bother him), playfulness (zoomies). Hermes moods colour the choice but
//! never override her: a sleeping Henry only flicks an ear (PLAN #2, #8).
//!
//! Body (M3): she stands on a surface (the taskbar of a monitor or the top edge of a window), flies with
//! gravity (thrown, jumping, falling), is held by the cursor, or climbs an outer screen edge.
use crate::win::{self};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use crate::tape::Tape;
use rand::SeedableRng;

mod animate;
mod decide;
mod motion;
mod pose;
mod present;
mod sense;
mod state;
mod world;

pub(crate) use pose::*;
pub(crate) use state::*;
pub(crate) use world::*;
pub(crate) use crate::bed::{BedMode, BedObj, drag_dir};

#[derive(Clone, Debug)]
pub struct SpriteInfo {
    pub cell_w: f64,
    pub cell_h: f64,
    /// First transparent row under the feet in the idle row (cell pixels).
    pub foot_y: f64,
}

/// Hermes agent state (same names and priority as hermes-agent `agent/pet/state.py`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Mood {
    Idle,
    Review,
    Run,
    Waiting,
    Wave,
    Jump,
    Failed,
}

impl Mood {
    pub fn salience(self) -> u8 {
        match self {
            Mood::Failed => 7,
            Mood::Jump => 6,
            Mood::Wave => 5,
            Mood::Waiting => 4,
            Mood::Run => 3,
            Mood::Review => 2,
            Mood::Idle => 0,
        }
    }
    /// One-shot beats: played once when they arrive, never a lasting mood.
    pub fn transient(self) -> bool {
        matches!(self, Mood::Wave | Mood::Jump | Mood::Failed)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Command {
    Sleep,
    Wake,
    Walk,
    Come,
    Wave,
    Zoomies,
    Idle,
    /// Groom now (scratch / wash art; idle if she has none).
    Groom,
    /// Stretch now (after-sleep stretch; she sits first if standing).
    Stretch,
    /// Jump onto a window top if one is in reach.
    Jump,
    /// Climb the nearest outer screen edge.
    Climb,
    /// Get down to the taskbar.
    Down,
    /// Go and drag a desktop icon a little (ignores the schedule, not the safety checks).
    Carry,
    /// Meow now (only when Sound is on).
    Meow,
    /// Greet the user as after a long absence: come to the cursor, meow there if calm (tests the real path).
    Greet,
    /// Go and lie down in her bed now (if the bed is out).
    Bed,
    /// Drag her bed somewhere else (in her teeth).
    Movebed,
    /// Go and nap on the window of the busiest GPU process now, even if it is barely warm (tests).
    Warm,
    /// Show the petting halo for 3 s as if she were being petted (tests; no purr, no bond).
    Glow,
}

/// Bed requests from the API.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum BedCmd {
    /// Drop the bed at screen x (None: next to Henry); it falls onto whatever is below.
    Place(Option<f64>),
}

pub struct Control {
    pub hidden: bool,
    /// On-screen height of one sprite cell in px (tray "Size").
    pub size: u32,
    pub quiet_apps: Vec<String>,
    /// Lasting moods per source (e.g. one per Hermes session) with expiry.
    pub moods: HashMap<String, (Mood, Instant)>,
    /// Transient beats waiting to be played.
    pub beats: VecDeque<Mood>,
    pub commands: VecDeque<Command>,
    /// Private memory of her WebView2 processes (MB), from the memory fuse.
    pub webview_mb: u64,
    pub sound: bool,
    pub mischief: bool,
    pub icons_path: PathBuf,
    /// Sprite set id (tray "Skin"); the brain reloads the sheet when it changes.
    pub skin: String,
    pub status: serde_json::Value,
    /// Her bed is out (tray "Bed").
    pub bed: bool,
    pub bed_cmds: VecDeque<BedCmd>,
    /// POST /bond: set her bond with the user (0..1).
    pub bond_set: Option<f64>,
    /// Recent mood/beat/act events for /status (debugging what she actually did).
    pub log: VecDeque<String>,
    pub started: Instant,
    /// POST /needs: set sleepiness / boredom / playfulness (0..1; None = keep).
    pub needs_set: Option<[Option<f64>; 3]>,
    /// Tray icon hover (tray events) for the status card (PLAN #54).
    pub tray: crate::card::TrayHover,
    /// API: pin / unpin the status card and/or move it (x, y)
    pub card_pin: Option<(Option<bool>, Option<(i32, i32)>)>,
    /// Notes written so far (the replay trace picks up the new ones each tick).
    pub noted: u64,
}

impl Control {
    pub fn new(hidden: bool, size: u32, quiet_apps: Vec<String>) -> Self {
        Self {
            hidden,
            size,
            quiet_apps,
            moods: HashMap::new(),
            beats: VecDeque::new(),
            commands: VecDeque::new(),
            webview_mb: 0,
            sound: false,
            mischief: false,
            icons_path: PathBuf::new(),
            skin: String::new(),
            status: json!({"act": "starting"}),
            card_pin: None,
            bed: true,
            bed_cmds: VecDeque::new(),
            bond_set: None,
            log: VecDeque::new(),
            started: Instant::now(),
            needs_set: None,
            tray: Default::default(),
            noted: 0,
        }
    }

    pub fn note(&mut self, s: String) {
        self.noted += 1;
        let t = self.started.elapsed().as_secs_f64();
        self.log.push_back(format!("{t:.1}s {s}"));
        while self.log.len() > 30 {
            self.log.pop_front();
        }
    }

    pub fn set_mood(&mut self, source: String, mood: Mood, ttl: Duration) {
        self.note(format!("mood {mood:?} from {source}"));
        if mood.transient() {
            if self.beats.len() < 4 {
                self.beats.push_back(mood);
            }
            self.moods.remove(&source);
        } else if mood == Mood::Idle {
            self.moods.remove(&source);
        } else {
            self.moods.insert(source, (mood, Instant::now() + ttl));
        }
    }

    /// Most salient lasting mood across live sources.
    pub fn mood(&mut self) -> Mood {
        let now = Instant::now();
        self.moods.retain(|_, (_, until)| *until > now);
        self.moods.values().map(|(m, _)| *m).max_by_key(|m| m.salience()).unwrap_or(Mood::Idle)
    }
}

pub fn spawn(ctl: Arc<Mutex<Control>>) {
    let dir = ctl.lock().unwrap().icons_path.parent().map(PathBuf::from).unwrap_or_default();
    // recording: the live run writes the same per-tick trace a replay does (they must match)
    let live_trace = std::env::var("HENRY_RECORD").ok().and_then(|d| {
        let _ = std::fs::create_dir_all(&d);
        std::fs::File::create(PathBuf::from(d).join("live-trace.txt")).ok()
    });
    let live_trace = live_trace.map(std::io::BufWriter::new);
    std::thread::spawn(move || run(ctl, Tape::live(&dir), live_trace));
}

/// Replay a recording (tape.rs) headless and write the per-tick trace to `out`.
pub fn replay(dir: &std::path::Path, out: &std::path::Path) {
    win::HEADLESS.store(true, std::sync::atomic::Ordering::Relaxed);
    let tape = Tape::replay(dir);
    // her state files are rewritten as she goes: work on copies, the recording stays as it was
    let work = dir.join("replay-state");
    let _ = std::fs::create_dir_all(&work);
    for f in crate::tape::STATE_FILES {
        let _ = std::fs::remove_file(work.join(f));
        let _ = std::fs::copy(dir.join(f), work.join(f));
    }
    let h = tape.header.clone().unwrap();
    let mut c = Control::new(false, 128, Vec::new());
    c.icons_path = work.join("icons.json");
    c.skin = h.skin.clone();
    let trace = std::io::BufWriter::new(std::fs::File::create(out).expect("trace file"));
    run(Arc::new(Mutex::new(c)), tape, Some(trace));
}

/// At most one meow per this long (PLAN #55).
const MEOW_GAP: Duration = Duration::from_secs(2 * 3600);

/// Warm spot (PLAN #52): a windowed process whose GPU load averaged this over 20 s draws her to its window.
const WARM_ON: f64 = 40.0;

/// ...and it counts as cooled (render done) when the last 6 s average is below this.
const WARM_OFF: f64 = 12.0;

fn run(ctl: Arc<Mutex<Control>>, tape: Tape, replay_trace: Option<std::io::BufWriter<std::fs::File>>) {
    let mut b = Brain::new(ctl, tape, replay_trace);
    while let Some(now) = b.tape.next_tick() {
        b.tick(now);
    }
}

// How long a one-shot animation of Henry's own sheet plays (else the default for the stand-in).
fn beat_dur(sheet: &crate::render::Sheet, name: &str, default: u64) -> Duration {
    Duration::from_millis(sheet.once_ms(name).map_or(default, |m| m as u64))
}

/// Spontaneous hops (onto a window / down from one) only with jump or fall art: without it the
/// flight draws the idle pose sliding through the air (the user: "she hops while sitting").
fn leap_art(sh: &crate::render::Sheet) -> bool {
    ["jump", "fall", "fly"].iter().any(|g| sh.has(g))
}

/// Values of one tick that more than one phase uses (frame-start snapshots: `grounded` is where she was
/// when the tick began, even after a phase launched her).
struct Tick {
    now: Instant,
    raw_dt: f64,
    dt: f64,
    icons_path: PathBuf,
    hidden: bool,
    size: u32,
    quiet_apps: Vec<String>,
    mood: Mood,
    beat: Option<Mood>,
    cmd: Option<Command>,
    sound_on: bool,
    mischief: bool,
    webview_mb: u64,
    bed_on: bool,
    was_in_bed: bool,
    want_meow: bool,
    greet_cmd: bool,
    cursor: (i32, i32),
    cx: f64,
    cy: f64,
    lbutton: bool,
    grab: bool,
    k: f64,
    w: i32,
    h: i32,
    half_body: f64,
    bed_ready: bool,
    night: bool,
    away: bool,
    user_active: bool,
    grounded: bool,
    x_tick0: f64,
    speed: f64,
    gap: f64,
    name: &'static str,
    dir: i8,
    fb_ms: Option<u32>,
    still: bool,
    sel: crate::render::Pick,
    cycle: u32,
    rotated: bool,
    rotate: i32,
    lift: f64,
}

/// Everything Henry's brain thread owns.
struct Brain {
    ctl: Arc<Mutex<Control>>,
    tape: Tape,
    replay_trace: Option<std::io::BufWriter<std::fs::File>>,
    t0: Instant,
    bond: crate::bond::Bond,
    noted_seen: u64,
    sheet: crate::render::Sheet,
    sprite: SpriteInfo,
    overlay: crate::render::Overlay,
    card: crate::card::Card,
    halo: crate::halo::Halo,
    trace: Option<std::io::BufWriter<std::fs::File>>,
    trace_t0: Instant,
    picker: crate::render::Picker,
    hwnd: isize,
    has_takeoff: bool,
    can_leap: bool,
    time_scale: f64,
    rng: rand::rngs::StdRng,
    world: World,
    bd: Body,
    mind: Mind,
    ps: PoseState,
    scr: Screen,
    sense: Sense,
    last: Instant,
    job: Errands,
    touch: Touch,
    bed: BedObj,
    gpu: Option<Arc<Mutex<crate::gpu::Gpu>>>,
}

impl Brain {
    fn new(ctl: Arc<Mutex<Control>>, mut tape: Tape, replay_trace: Option<std::io::BufWriter<std::fs::File>>) -> Brain {
        let t0 = tape.t0;
        // bond with the user (PLAN #53); loaded first: the tape header carries its value
        let mut bond = crate::bond::Bond::load(ctl.lock().unwrap().icons_path.with_file_name("bond.json"));
        let header = tape.header(|| crate::tape::Header {
            seed: rand::thread_rng().gen(),
            time_scale: std::env::var("HENRY_TIME_SCALE").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0),
            start_x: std::env::var("HENRY_START_X").ok(),
            skin: ctl.lock().unwrap().skin.clone(),
            bond: bond.v,
        });
        bond.v = header.bond;
        bond.reset_clock(t0);
        let noted_seen = ctl.lock().unwrap().noted;
        let sheet = crate::render::Sheet::load(&ctl.lock().unwrap().skin.clone());
        let sprite = sheet.info.clone();
        let overlay = crate::render::Overlay::new();
        let card = crate::card::Card::new(ctl.lock().unwrap().icons_path.with_file_name("card.json"));
        let halo = crate::halo::Halo::new();
        // Dev knob: HENRY_TRACE=<file> logs every drawn tick (ms, anim, frame, window x, x, pin) for motion checks.
        let trace = std::env::var("HENRY_TRACE").ok().and_then(|p| std::fs::File::create(p).ok()).map(std::io::BufWriter::new);
        let trace_t0 = t0;
        let picker = crate::render::Picker::new();
        let hwnd = overlay.hwnd;
        let has_takeoff = sheet.has("jump");
        let can_leap = leap_art(&sheet);
        // Dev knob: HENRY_TIME_SCALE=60 makes needs grow 60x faster.
        let time_scale: f64 = header.time_scale;

        let mut rng = rand::rngs::StdRng::seed_from_u64(header.seed);
        let mons = tape.val("mons", win::monitors);
        let first = mons.iter().find(|m| m.primary).or(mons.first()).cloned().expect("a monitor");
        let world = World { mons, wins: tape.val("wins", || win::windows(hwnd)), h: 128.0 };
        let mut bd = Body {
            x: (first.work.left + first.work.right) as f64 / 2.0,
            y: first.work.bottom as f64,
            vx: 0.0,
            vy: 0.0,
            mode: Mode::Ground(Surface::Floor),
            mon: first.clone(),
            on_rect: None,
            face: 1,
            swing: None,
            samples: VecDeque::new(),
            cling_until: t0,
            held_since: t0,
            takeoff: None,
            perch_target: None,
        };
        let mut mind = Mind {
            act: Act::Idle,
            act_until: t0 + Duration::from_secs(2),
            needs: Needs { sleepiness: 0.2, boredom: 0.1, playfulness: 0.3 },
            last_mood: Mood::Idle,
            attended_waiting: false,
            greet: false,
            was_away: false,
            away_for: Duration::ZERO,
            nap_by_cursor: false,
            last_meow: t0 - MEOW_GAP,
        };
        // Dev knob: HENRY_START_X=<px>[,<dir>] starts her at x walking in dir (-1/1) for ~8 s (edge/monitor tests).
        if let Some((sx, dir)) = header.start_x.clone().and_then(|v| {
            let mut p = v.split(',');
            let sx = p.next()?.trim().parse::<f64>().ok()?;
            let dir = p.next().and_then(|d| d.trim().parse::<i8>().ok()).unwrap_or(-1).signum();
            Some((sx, dir))
        }) {
            bd.x = sx;
            mind.act = Act::Walk(if dir == 0 { -1 } else { dir });
            mind.act_until = t0 + Duration::from_secs(8);
        }

        let ps = PoseState::new(bd.x);
        let scr = Screen {
            shown: false,
            placed: (i32::MIN, 0, 0, 0),
            interactive: false,
            last_top: t0,
            last_status: t0,
            last_act_name: "",
            last_mode_name: "",
            glow_test: None,
        };
        let sense = Sense {
            fullscreen: None,
            quiet: false,
            user_idle: Duration::ZERO,
            hour: 12,
            hot: None,
            gpu_top: Vec::new(),
            gpu_ok: false,
            last_env: t0 - Duration::from_secs(1),
            last_wins: t0,
        };
        let last = t0;
        let job = Errands {
            carry: None,
            carry_asap: false,
            next_carry: t0 + Duration::from_secs_f64(rng.gen_range(1800.0..5400.0) / time_scale),
            bed_jump: false,
            bed_deadline: t0,
            fetch_tx: 0.0,
            warm: None,
            warm_tx: 0.0,
            warm_nap: false,
            warm_jump: false,
            warm_deadline: t0,
            warm_off: WARM_OFF,
            warm_force: false,
            cool_since: None,
            next_warm: t0 + Duration::from_secs(30),
            get_down: None,
        };
        let touch = Touch {
            stroke: 0.0,
            last_cur: (0.0, 0.0),
            last_stroke: t0,
            purring: false,
            prev_over: false,
            prev_lbutton: false,
            prev_over_bed: false,
            last_pet_bonus: t0 - Duration::from_secs(600),
            cur_speed: 0.0,
            last_bolt: t0,
            tossed: false,
        };
        // her bed (PLAN #51)
        let bed_path = ctl.lock().unwrap().icons_path.with_file_name("bed.json");
        let bed = BedObj::load(bed_path, &world.mons);
        // warm spot (PLAN #52): GPU sampler thread
        let gpu = if tape.replaying() { None } else { Some(crate::gpu::spawn()) };
        Brain { ctl, tape, replay_trace, t0, bond, noted_seen, sheet, sprite, overlay, card, halo, trace, trace_t0, picker, hwnd, has_takeoff, can_leap, time_scale, rng, world, bd, mind, ps, scr, sense, last, job, touch, bed, gpu }
    }

    fn tick(&mut self, now: Instant) {
        let mut tk = self.sense(now);
        self.decide(&mut tk);
        self.motion(&mut tk);
        self.animate(&mut tk);
        self.present(&mut tk);
    }
}
