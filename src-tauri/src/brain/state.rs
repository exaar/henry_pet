//! What Henry is: her acts and body modes, and the state the brain thread keeps between ticks.
use super::*;
use crate::win::{Monitor, Rect, WinInfo};
use serde::Serialize;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// What she does while standing on a surface.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Act {
    Idle,
    Walk(i8),
    Look,
    /// One-shot animation (wave, flop, jump) then idle.
    Beat(&'static str),
    ToCorner(i8),
    Rest,
    Sleep,
    /// Walk to the cursor's x, then attend.
    Come,
    /// Sit next to the cursor looking at the user.
    Attend,
    Zoomies(i8),
    /// Walk to an outer screen edge, then climb it.
    ToWall(i8),
    /// Walk toward an icon's launch point, then jump up next to it.
    Fetch,
    /// Drag the icon along its row.
    Carry,
    /// Walk to her bed and hop in (then sleep there).
    ToBed,
    /// Walk to her bed to pick it up and drag it in direction d.
    FetchBed(i8),
    /// Walking with the bed in her teeth.
    DragBed(i8),
    /// Off to nap on the warm window (a render / game loading the GPU, PLAN #52).
    ToWarm,
}

impl Act {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Act::Idle => "idle",
            Act::Walk(_) => "walk",
            Act::Look => "look",
            Act::Beat(_) => "beat",
            Act::ToCorner(_) => "to-corner",
            Act::Rest => "rest",
            Act::Sleep => "sleep",
            Act::Come => "come",
            Act::Attend => "attend",
            Act::Zoomies(_) => "zoomies",
            Act::ToWall(_) => "to-wall",
            Act::Fetch => "fetch",
            Act::Carry => "carry",
            Act::ToBed => "to-bed",
            Act::FetchBed(_) => "fetch-bed",
            Act::DragBed(_) => "drag-bed",
            Act::ToWarm => "to-warm",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Surface {
    Floor,
    Window(isize),
    /// The row of the icon she is dragging (screen y of her feet).
    Perch(i32),
    /// In her bed (it carries her wherever it is).
    Bed,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Mode {
    Ground(Surface),
    Air,
    /// Held by the cursor; offset = feet minus cursor at the grab.
    Held(f64, f64),
    /// Climbing an outer edge (side +1 = right wall) up to target y; y is her body centre here.
    Climb(i8, f64),
    Cling(i8),
}

impl Mode {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Mode::Ground(Surface::Floor) => "floor",
            Mode::Ground(Surface::Window(_)) => "window",
            Mode::Ground(Surface::Perch(_)) => "perch",
            Mode::Ground(Surface::Bed) => "bed",
            Mode::Air => "air",
            Mode::Held(..) => "held",
            Mode::Climb(..) => "climb",
            Mode::Cling(_) => "cling",
        }
    }
}

#[derive(Default, Clone, Copy, Debug, Serialize)]
pub(crate) struct Needs {
    pub(crate) sleepiness: f64,
    pub(crate) boredom: f64,
    pub(crate) playfulness: f64,
}

/// Where her body is and how it moves (run() state, PLAN refactor).
pub(crate) struct Body {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) vx: f64,
    pub(crate) vy: f64,
    pub(crate) mode: Mode,
    pub(crate) mon: Monitor,
    pub(crate) on_rect: Option<Rect>,
    /// facing for undirected animations (she keeps looking where she last went)
    pub(crate) face: i8,
    /// held by the scruff with `held` art that has a pivot: soft body chain (slinky.rs)
    pub(crate) swing: Option<crate::slinky::Slinky>,
    /// cursor samples while held (throw velocity)
    pub(crate) samples: VecDeque<(Instant, f64, f64)>,
    pub(crate) cling_until: Instant,
    pub(crate) held_since: Instant,
    /// jump decided, crouching for the take-off animation: target top
    pub(crate) takeoff: Option<(f64, f64)>,
    pub(crate) perch_target: Option<(f64, f64)>,
}

/// What she is doing and wants.
pub(crate) struct Mind {
    pub(crate) act: Act,
    pub(crate) act_until: Instant,
    pub(crate) needs: Needs,
    pub(crate) last_mood: Mood,
    pub(crate) attended_waiting: bool,
    /// she is on her way to greet the user after a long absence: meow when she gets there
    pub(crate) greet: bool,
    pub(crate) was_away: bool,
    pub(crate) away_for: Duration,
    pub(crate) nap_by_cursor: bool,
    pub(crate) last_meow: Instant,
}

/// The user's hand: petting, bolting, grabs.
pub(crate) struct Touch {
    pub(crate) stroke: f64,
    pub(crate) last_cur: (f64, f64),
    pub(crate) last_stroke: Instant,
    pub(crate) purring: bool,
    pub(crate) prev_over: bool,
    pub(crate) prev_lbutton: bool,
    pub(crate) prev_over_bed: bool,
    pub(crate) last_pet_bonus: Instant,
    pub(crate) cur_speed: f64,
    pub(crate) last_bolt: Instant,
    /// Last release was a real throw (PLAN #64): a splat right after it costs bond, her own falls don't.
    pub(crate) tossed: bool,
}

/// What she knows about the desktop beyond the window list (polled).
pub(crate) struct Sense {
    pub(crate) fullscreen: Option<isize>,
    pub(crate) quiet: bool,
    pub(crate) user_idle: Duration,
    pub(crate) hour: u32,
    pub(crate) hot: Option<(WinInfo, f64)>,
    pub(crate) gpu_top: Vec<(u32, f64)>,
    pub(crate) gpu_ok: bool,
    pub(crate) last_env: Instant,
    pub(crate) last_wins: Instant,
}

/// Errands that span many ticks: icon carry, bed trips, the warm spot.
pub(crate) struct Errands {
    pub(crate) carry: Option<CarryPlan>,
    pub(crate) carry_asap: bool,
    pub(crate) next_carry: Instant,
    /// the jump under way is into the bed
    pub(crate) bed_jump: bool,
    pub(crate) bed_deadline: Instant,
    pub(crate) fetch_tx: f64,
    /// warm spot (PLAN #52): the window she is going to / napping on
    pub(crate) warm: Option<(isize, u32)>,
    pub(crate) warm_tx: f64,
    pub(crate) warm_nap: bool,
    pub(crate) warm_jump: bool,
    pub(crate) warm_deadline: Instant,
    pub(crate) warm_off: f64,
    pub(crate) warm_force: bool,
    pub(crate) cool_since: Option<Instant>,
    pub(crate) next_warm: Instant,
    /// getting down off the window she is on: walk to this edge and hop off it (never turn back there)
    pub(crate) get_down: Option<i8>,
}

/// What is on screen.
pub(crate) struct Screen {
    pub(crate) shown: bool,
    pub(crate) placed: (i32, i32, i32, i32),
    pub(crate) interactive: bool,
    pub(crate) last_top: Instant,
    pub(crate) last_status: Instant,
    pub(crate) last_act_name: &'static str,
    pub(crate) last_mode_name: &'static str,
    pub(crate) glow_test: Option<Instant>,
}
