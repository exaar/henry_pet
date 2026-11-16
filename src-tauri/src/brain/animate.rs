//! Phase 4: which animation - logical name + direction, the picker, pose transitions; the bed under her paws.
use super::*;
use std::time::Duration;

impl Brain {
    /// Which animation: logical name + direction, the picker, pose transitions; the bed under her paws.
    pub(super) fn animate(&mut self, tk: &mut Tick) {
        if self.mind.act.name() != self.scr.last_act_name || self.bd.mode.name() != self.scr.last_mode_name {
            self.scr.last_act_name = self.mind.act.name();
            self.scr.last_mode_name = self.bd.mode.name();
            self.ctl.lock().unwrap().note(format!("{} / {:?}", self.bd.mode.name(), self.mind.act));
        }

        // --- animation: logical name + direction; the picker resolves fallbacks, variants, mirroring ---
        if self.mind.act != self.ps.epoch_act || self.bd.mode.name() != self.ps.epoch_mode {
            self.ps.epoch_act = self.mind.act;
            self.ps.epoch_mode = self.bd.mode.name();
            self.ps.epoch += 1;
        }
        let dir_of = |v: f64| if v < 0.0 { -1i8 } else { 1i8 };
        let twitching = self.ps.twitch_until.is_some_and(|t| t > tk.now);
        // (name, direction, cycle ms when it falls back to a stand-in, hold first frame)
        (tk.name, tk.dir, tk.fb_ms, tk.still) = match self.bd.mode {
            Mode::Held(..) => ("held", self.bd.face, None, false),
            Mode::Air => (if self.bd.vy < 0.0 { "fly" } else { "fall" }, if self.bd.vx.abs() > 30.0 { dir_of(self.bd.vx) } else { self.bd.face }, None, false),
            Mode::Climb(d, _) => ("climb", d, Some(900), false),
            Mode::Cling(d) => ("climb", d, None, true),
            Mode::Ground(_) => match self.mind.act {
                Act::Walk(d) | Act::ToCorner(d) | Act::ToWall(d) => ("walk", d, None, false),
                Act::Fetch => ("walk", dir_of(self.job.carry.as_ref().map_or(1.0, |c| c.lx - self.bd.x)), None, false),
                Act::ToBed => ("walk", dir_of(self.bed.x - self.bd.x), None, false),
                Act::ToWarm => ("walk", dir_of(self.job.warm_tx - self.bd.x), None, false),
                Act::FetchBed(d) => ("walk", if (self.job.fetch_tx - self.bd.x).abs() < 4.0 { d } else { dir_of(self.job.fetch_tx - self.bd.x) }, None, false),
                Act::DragBed(d) => ("walk", d, None, false),
                Act::Carry => ("push", self.job.carry.as_ref().map_or(self.bd.face, |c| c.d), Some(1500), false),
                Act::Zoomies(d) => ("trot", d, Some(500), false),
                Act::Come => ("trot", dir_of(tk.cx - self.bd.x), Some(800), false),
                Act::Look => ("look", self.bd.face, None, false),
                Act::Beat(r) => (r, self.bd.face, None, false),
                Act::Attend => (if self.touch.purring { "petted" } else { "stare" }, self.bd.face, None, false),
                Act::Sleep => (if twitching { "twitch" } else { "sleep" }, self.bd.face, None, false),
                Act::Idle => ("idle", self.bd.face, None, false),
                Act::Rest => ("rest", self.bd.face, None, false),
            },
        };
        self.bd.face = tk.dir;
        tk.sel = self.picker.pick(&self.sheet, tk.name, tk.dir, self.ps.epoch, self.ps.pose, &mut self.rng);
        // a one-shot beat lasts as long as the variant actually picked (variants differ: scratch x1..x3)
        if matches!(self.mind.act, Act::Beat(_)) && self.ps.beat_epoch != self.ps.epoch {
            self.ps.beat_epoch = self.ps.epoch;
            let a = &self.sheet.anims[tk.sel.anim];
            if !tk.sel.fallback && a.once {
                self.mind.act_until = tk.now + Duration::from_millis(a.ms as u64);
                // a sit-down / get-up already under way plays out first: the beat starts after it
                // (else a jump picked mid-get-up launched before its crouch was ever drawn)
                if let Some(left) = self.ps.clips_left(tk.now, &self.sheet) {
                    self.mind.act_until += left;
                }
            }
        }
        // pose changes on the ground play a transition first (she holds still), then the picked animation
        // (pose.rs: ROUTES, LEAD_INS)
        let pin = PoseIn { sheet: &self.sheet, now: tk.now, grounded: matches!(self.bd.mode, Mode::Ground(_)), dir: tk.dir, x_tick0: tk.x_tick0, k: tk.k };
        self.ps.step(&pin, &mut tk.sel, &mut self.bd.x, &mut self.mind.act_until, &mut self.rng);
        tk.cycle = {
            let own = self.sheet.anims[tk.sel.anim].ms;
            if tk.sel.fallback { tk.fb_ms.unwrap_or(own) } else { own }
        };
        // no climb art of her own: the walk is turned 90° against the wall
        tk.rotated = matches!(self.bd.mode, Mode::Climb(..) | Mode::Cling(_)) && tk.sel.fallback;
        tk.rotate = match self.bd.mode {
            Mode::Climb(d, _) | Mode::Cling(d) if tk.rotated => if d > 0 { -90 } else { 90 },
            _ => 0,
        };

        // walking over the bed on the same surface: she steps up onto it and it gives under her paws
        tk.lift = if matches!(self.bd.mode, Mode::Ground(Surface::Floor) | Mode::Ground(Surface::Window(_))) && tk.bed_on && self.bed.grounded() {
            self.bed.lift(self.bd.x, self.bd.y, tk.k).unwrap_or(0.0)
        } else {
            0.0
        };
        if tk.lift > 0.0 {
            self.bed.jelly.press(self.bed.u(self.bd.x, tk.k), 0.3, 3.0);
        }
        if self.bd.mode == Mode::Ground(Surface::Bed) {
            if self.ps.pose == crate::render::Pose::Lie {
                self.bed.jelly.press(0.0, 0.6, 3.0);
            } else {
                self.bed.jelly.press(0.0, 0.35, 4.0);
            }
        }
        if tk.was_in_bed && self.bd.mode != Mode::Ground(Surface::Bed) {
            self.bed.jelly.kick(0.0, 0.6, -45.0, -1.2); // she pushed off: it springs back
        }

    }
}
