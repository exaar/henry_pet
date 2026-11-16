//! Her bond with the user (PLAN #53, reworked #64): one slow number 0..1, kept across runs in `bond.json`.
//! The user's company (at the PC while she is out) and petting raise it; only rough handling lowers it:
//! being pulled out of sleep, real throws and the splat after one. Carrying her around is how she gets moved,
//! not hostility, and her own falls are nobody's fault. Company alone tops out at COMPANY_CAP ("good");
//! closer than that takes petting. It drifts back toward neutral by 1 % a day, so neither extreme is forever.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Where she starts and drifts back to: the middle of the legacy bot's "neutral" level.
pub const NEUTRAL: f64 = 0.42;
const DRIFT_PER_DAY: f64 = 0.01;
/// Company: per hour of the user active at the PC while she is out, up to the cap.
const COMPANY_PER_HOUR: f64 = 0.01;
pub const COMPANY_CAP: f64 = 0.6;

pub const PET: f64 = 0.03;
pub const PET_AGAIN: f64 = 0.008;
/// Picked up while asleep (woken).
pub const WOKEN: f64 = -0.01;
/// Let go faster than THROW_SPEED (px/s, |vx|+|vy|): a real throw, not a carry.
pub const THROWN: f64 = -0.015;
pub const THROW_SPEED: f64 = 2500.0;
/// Hard landing right after a throw.
pub const SPLAT: f64 = -0.01;

#[derive(Serialize, Deserialize)]
struct File {
    bond: f64,
    /// unix seconds of the last save (drift while she was not running)
    updated: u64,
}

pub struct Bond {
    pub v: f64,
    path: PathBuf,
    dirty: bool,
    last_save: Instant,
    last_drift: Instant,
}

fn unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn drift(v: f64, days: f64) -> f64 {
    let step = DRIFT_PER_DAY * days;
    if v > NEUTRAL {
        (v - step).max(NEUTRAL)
    } else {
        (v + step).min(NEUTRAL)
    }
}

impl Bond {
    pub fn load(path: PathBuf) -> Self {
        let v = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<File>(&s).ok())
            .map_or(NEUTRAL, |f| drift(f.bond.clamp(0.0, 1.0), unix().saturating_sub(f.updated) as f64 / 86400.0));
        Bond { v, path, dirty: false, last_save: Instant::now(), last_drift: Instant::now() }
    }

    /// Replay: start her clocks at the tape's start (load took Instant::now()).
    pub fn reset_clock(&mut self, t: Instant) {
        self.last_save = t;
        self.last_drift = t;
    }

    pub fn add(&mut self, d: f64) {
        self.v = (self.v + d).clamp(0.0, 1.0);
        self.dirty = true;
    }

    /// The user's company for `secs` (only below the cap; never pulls a closer cat down).
    pub fn company(&mut self, secs: f64) {
        if self.v < COMPANY_CAP {
            self.v = (self.v + secs / 3600.0 * COMPANY_PER_HOUR).min(COMPANY_CAP);
            self.dirty = true;
        }
    }

    pub fn set(&mut self, v: f64) {
        self.v = v.clamp(0.0, 1.0);
        self.dirty = true;
    }

    /// Call every frame: drift + save at most every 30 s when changed.
    pub fn tick(&mut self, now: Instant) {
        if now - self.last_drift > Duration::from_secs(60) {
            let days = (now - self.last_drift).as_secs_f64() / 86400.0;
            self.last_drift = now;
            let nv = drift(self.v, days);
            if nv != self.v {
                self.v = nv;
                self.dirty = true;
            }
        }
        if self.dirty && now - self.last_save > Duration::from_secs(30) {
            self.save();
            self.last_save = now;
        }
    }

    pub fn save(&mut self) {
        let f = File { bond: self.v, updated: unix() };
        if let Ok(s) = serde_json::to_string(&f) {
            let _ = std::fs::write(&self.path, s);
        }
        self.dirty = false;
    }

    /// The legacy bot's six levels.
    pub fn level(&self) -> &'static str {
        match self.v {
            v if v < 1.0 / 6.0 => "awful",
            v if v < 2.0 / 6.0 => "bad",
            v if v < 3.0 / 6.0 => "neutral",
            v if v < 4.0 / 6.0 => "good",
            v if v < 5.0 / 6.0 => "amazing",
            _ => "owner",
        }
    }

    /// Wary of the cursor: bolts when it rushes at her.
    pub fn wary(&self) -> bool {
        self.v < 0.3
    }

    /// Close: sometimes naps right by the cursor.
    pub fn close(&self) -> bool {
        self.v > 0.75
    }
}
