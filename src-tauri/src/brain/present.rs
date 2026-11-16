//! Phase 5: on screen - her window, the bed, clicks, petting, bolting, meow, /status, halo, card, replay trace.
use super::*;
use crate::win::{self};
use serde_json::json;
use std::time::Duration;

/// What the status card says she is doing (5x7 font, at most 15 characters).
pub(crate) fn activity(act: Act, mode: Mode, purring: bool, warm_nap: bool, pose: crate::render::Pose) -> &'static str {
    match mode {
        Mode::Held(..) => return "HELD BY YOU",
        Mode::Air => return "AIRBORNE!",
        Mode::Climb(..) => return "CLIMBING",
        Mode::Cling(_) => return "HANGING ON",
        Mode::Ground(Surface::Bed) if act == Act::Sleep => return "SLEEPING IN BED",
        Mode::Ground(Surface::Bed) => return "IN HER BED",
        _ => {}
    }
    match act {
        Act::Idle if pose == crate::render::Pose::Stand => "STANDING",
        Act::Idle => "SITTING",
        Act::Walk(_) => "WALKING",
        Act::Look => "LOOKING AROUND",
        Act::Beat("wave") => "SAYING HI",
        Act::Beat("groom") => "GROOMING",
        Act::Beat("stretch") => "STRETCHING",
        Act::Beat("jump") | Act::Beat("hop") => "JUMPING",
        Act::Beat("land") => "LANDING",
        Act::Beat("splat") => "SPLAT!",
        Act::Beat("failed") => "OOPS",
        Act::Beat(_) => "BUSY",
        Act::ToCorner(_) => "MOVING ASIDE",
        Act::Rest => "LYING LOW",
        Act::Sleep if warm_nap => "WARM NAP",
        Act::Sleep => "SLEEPING",
        Act::Come => "COMING TO YOU",
        Act::Attend if purring => "PURRING",
        Act::Attend => "WATCHING YOU",
        Act::Zoomies(_) => "ZOOMIES!",
        Act::ToWall(_) => "OFF TO CLIMB",
        Act::Fetch => "UP TO MISCHIEF",
        Act::Carry => "PUSHING AN ICON",
        Act::ToBed => "GOING TO BED",
        Act::FetchBed(_) | Act::DragBed(_) => "MOVING HER BED",
        Act::ToWarm => "TO A WARM SPOT",
    }
}

impl Brain {
    /// On screen: her window, the bed, clicks, petting, bolting, meow, /status, halo, card, replay trace.
    pub(super) fn present(&mut self, tk: &mut Tick) {
        // --- place the window ---
        // leaving a paw-pinned walk: the drawing stood at x + pin, so she stops right there (else the next
        // animation pops forward by up to one step)
        if tk.sel.anim != self.ps.last_drawn && self.ps.last_pin != 0.0 && matches!(self.bd.mode, Mode::Ground(_)) {
            self.bd.x += self.ps.last_pin;
            self.ps.x_prev = self.bd.x;
        }
        let (wx, wy, ww, wh) = match self.bd.mode {
            Mode::Climb(d, _) | Mode::Cling(d) if !tk.rotated => {
                // own climb art: drawn upright, paws on the cell's right edge (mirrored for the left wall)
                let wx = if d > 0 { self.bd.x - tk.w as f64 } else { self.bd.x };
                (wx.round() as i32, (self.bd.y - tk.h as f64 / 2.0).round() as i32, tk.w, tk.h)
            }
            Mode::Climb(d, _) | Mode::Cling(d) => {
                // rotated: window is h wide and w tall, feet against the wall
                let wall = self.bd.x;
                let wx = if d > 0 { wall + tk.gap - tk.h as f64 } else { wall - tk.gap };
                (wx.round() as i32, (self.bd.y - tk.w as f64 / 2.0).round() as i32, tk.h, tk.w)
            }
            _ => ((self.bd.x - tk.w as f64 / 2.0).round() as i32, (self.bd.y - self.sprite.foot_y * tk.k - tk.lift).round() as i32, tk.w, tk.h),
        };
        let my_mon = monitor_at(&self.world.mons, self.bd.x, self.bd.y - 2.0).map(|m| m.handle);
        let visible = !tk.hidden && (my_mon != self.sense.fullscreen || self.sense.fullscreen.is_none());
        // walk with steps: the frame comes from the distance she actually covered (sum of steps = one cycle),
        // at any speed - run x1.3, sleepy x0.75, zoomies on the walk stand-in; a fixed 2 s cycle under a
        // 110 px/s body was an escalator glide. The body itself moves smoothly every refresh: pinning the
        // drawing to the planted paw (PLAN #58/#59) made her stand still and hop a whole step per frame,
        // on an uneven beat - stop-motion. `pin` stays 0 (kept for the trace / stop bookkeeping).
        let pin = 0.0;
        self.overlay.force = None;
        {
            let a = &self.sheet.anims[tk.sel.anim];
            let cycle_px: f64 = a.steps.iter().sum();
            if a.steps.len() == a.seq.len() && cycle_px > 0.0 && matches!(self.bd.mode, Mode::Ground(_)) && !tk.rotated && !tk.still {
                if tk.sel.anim != self.ps.last_drawn {
                    self.ps.walk_dist = 0.0; // a new walk starts on its first cell (trans_walk hands over on it)
                } else {
                    self.ps.walk_dist = (self.ps.walk_dist + ((self.bd.x - self.ps.x_prev).abs() / tk.k).min(cycle_px / 4.0)).rem_euclid(cycle_px);
                }
                // even spacing: the source run was filmed at a constant frame rate, each frame is the same
                // slice of the cycle (the measured per-frame `steps` are paw-overlap noise, 3..7 px, and as
                // frame boundaries they made the cadence and the body's speed lurch frame to frame)
                let n = a.steps.len();
                self.overlay.force = Some(((self.ps.walk_dist / cycle_px * n as f64) as usize).min(n - 1));
            }
        }
        self.ps.last_drawn = tk.sel.anim;
        self.ps.last_pin = pin;
        self.ps.x_prev = self.bd.x;
        let wx = wx + pin.round() as i32;
        if visible {
            match (self.bd.mode, self.bd.swing.as_ref(), self.sheet.anims[tk.sel.anim].pivot) {
                (Mode::Held(..), Some(sw), Some(_)) => {
                    self.scr.placed = self.overlay.render_slinky(tk.now, &self.sheet, tk.sel, tk.cycle, sw, tk.w, tk.h);
                }
                _ => {
                    self.scr.placed = (wx, wy, ww, wh);
                    self.overlay.render(tk.now, &self.sheet, tk.sel, tk.cycle, tk.still, tk.rotate, wx, wy, tk.w, tk.h);
                }
            }
            if let Some(tf) = self.trace.as_mut() {
                use std::io::Write;
                let _ = writeln!(tf, "{:.1}\t{}\t{}\t{}\t{:.2}\t{:.2}", (tk.now - self.trace_t0).as_secs_f64() * 1000.0, self.sheet.anims[tk.sel.anim].name, self.overlay.frame, self.scr.placed.0, self.bd.x, pin);
                let _ = tf.flush();
            }
        }
        if visible != self.scr.shown {
            self.scr.shown = visible;
            win::set_visible(self.hwnd, visible);
            if visible {
                win::topmost(self.hwnd);
                self.card.raise();
            }
        }
        if self.scr.shown && tk.now - self.scr.last_top > Duration::from_millis(1500) {
            self.scr.last_top = tk.now;
            win::topmost(self.hwnd);
            self.bed.view.order(self.hwnd);
            self.halo.order(self.hwnd);
            self.card.raise();
        }

        // --- the bed: in her teeth only while she drags / jumps with it; move it; draw it ---
        let carry_ok = match self.bed.mode {
            BedMode::Carried => {
                tk.bed_on
                    && !self.sense.quiet
                    && ((matches!(self.mind.act, Act::DragBed(_)) && self.bd.mode == Mode::Ground(Surface::Floor))
                        || self.bd.mode == Mode::Air
                        || (self.mind.act == Act::Beat("jump") && self.bd.takeoff.is_some()))
            }
            _ => true,
        };
        if !carry_ok {
            self.bed.drop_it(self.bd.vx * 0.5, self.bd.vy.min(0.0) * 0.5);
            self.ctl.lock().unwrap().note("bed let go".into());
        }
        let mouth_at = {
            let an = &self.sheet.anims[tk.sel.anim];
            let (mx, my) = mouth(&an.name);
            let facing_left = (an.facing < 0) != tk.sel.mirror;
            let mx = if facing_left { self.sprite.cell_w - mx } else { mx };
            (self.scr.placed.0 as f64 + mx * tk.k, self.scr.placed.1 as f64 + my * tk.k)
        };
        {
            let floor = if matches!(self.bd.mode, Mode::Ground(_)) { Some(self.bd.y) } else { None };
            let mut notes = Vec::new();
            self.bed.step(&self.world, tk.k, tk.dt, tk.now, (tk.cx, tk.cy), tk.lbutton, mouth_at, floor, &mut notes);
            if !notes.is_empty() {
                let mut c = self.ctl.lock().unwrap();
                for n in notes {
                    c.note(n);
                }
            }
        }
        let bed_mon = monitor_at(&self.world.mons, self.bed.x, self.bed.y - 2.0).map(|m| m.handle);
        let bed_visible = tk.bed_on && !tk.hidden && (self.sense.fullscreen.is_none() || bed_mon != self.sense.fullscreen);
        let layers_before = (self.bed.view.back.shown, self.bed.view.front.shown);
        if bed_visible {
            match self.bed.mode {
                BedMode::Held(left_up) => {
                    if let Some(sw) = self.bed.swing.as_ref() {
                        self.bed.view.held(&self.bed.jelly, sw, left_up, tk.k);
                    }
                }
                BedMode::Carried => {
                    if let Some(d) = self.bed.dangle.as_ref() {
                        self.bed.view.rotated(&self.bed.jelly, d, tk.k);
                    }
                }
                _ => {
                    // her dangling paw (lying art, below the belly, ahead of the body) hangs OVER the front rim
                    // whatever frame is drawn right now - lying, lying down, getting up: the paw shows up mid
                    // trans_lie and must hang over the rim from the first frame it appears
                    let an = tk.sel.anim;
                    let nm = &self.sheet.anims[an].name;
                    let lying = self.sheet.anims[an].pose == crate::render::Pose::Lie || nm == "trans_lie" || nm.starts_with("trans_unlie");
                    let fr = self.overlay.frame;
                    let facing_left = (self.sheet.anims[an].facing < 0) != tk.sel.mirror;
                    let (cw, foot) = (self.sprite.cell_w, self.sprite.foot_y);
                    let pl = self.scr.placed;
                    let paw = |sx: i32, sy: i32| -> bool {
                        if !lying {
                            return false;
                        }
                        let u = ((sx - pl.0) as f64 / tk.k).floor();
                        let v = ((sy - pl.1) as f64 / tk.k).floor();
                        let u = if facing_left { cw - 1.0 - u } else { u };
                        v >= foot - 2.0 && u >= cw * 0.58 && self.sheet.opaque_at(an, fr, u as i32, v as i32)
                    };
                    self.bed.view.upright(&self.bed.jelly, self.bed.x, self.bed.y, tk.k, self.bd.mode == Mode::Ground(Surface::Bed), &paw);
                }
            }
        } else {
            self.bed.view.hide();
        }
        if layers_before != (self.bed.view.back.shown, self.bed.view.front.shown) {
            self.bed.view.order(self.hwnd);
            self.halo.order(self.hwnd);
            self.card.raise();
        }

        // --- clicks: catch the mouse only while it is over her body ---
        let over = {
            let (px, py, pw, ph) = (self.scr.placed.0 as f64, self.scr.placed.1 as f64, self.scr.placed.2 as f64, self.scr.placed.3 as f64);
            tk.cx >= px + pw * 0.18 && tk.cx < px + pw * 0.82 && tk.cy >= py + ph * 0.2 && tk.cy < py + ph * 0.92
        };
        let over_bed = bed_visible && !over && self.bed.view.hit(tk.cx as i32, tk.cy as i32);
        self.bed.view.set_interactive(over_bed || matches!(self.bed.mode, BedMode::Held(_)));
        self.touch.prev_over_bed = over_bed;
        let want_inter = self.scr.shown && (over || matches!(self.bd.mode, Mode::Held(..)));
        if want_inter != self.scr.interactive {
            self.scr.interactive = want_inter;
            win::set_click_through(self.hwnd, !self.scr.interactive);
        }

        // --- wary (low bond): the cursor rushing at her makes her bolt; a slow hand can still pet her ---
        if tk.dt > 0.0 {
            let inst = ((tk.cx - self.touch.last_cur.0).powi(2) + (tk.cy - self.touch.last_cur.1).powi(2)).sqrt() / tk.dt;
            self.touch.cur_speed += (inst - self.touch.cur_speed) * (tk.dt * 12.0).min(1.0);
        }
        if self.bond.wary()
            && !self.sense.quiet
            && tk.now - self.touch.last_bolt > Duration::from_secs(5)
            && matches!(self.bd.mode, Mode::Ground(Surface::Floor) | Mode::Ground(Surface::Window(_)))
            && matches!(self.mind.act, Act::Idle | Act::Walk(_) | Act::Look | Act::Attend)
            && self.bd.takeoff.is_none()
            && !self.touch.purring
        {
            let body_y = self.bd.y - tk.h as f64 * 0.35;
            let dist = ((tk.cx - self.bd.x).powi(2) + (tk.cy - body_y).powi(2)).sqrt();
            let closing = (tk.cx - self.bd.x).abs() < (self.touch.last_cur.0 - self.bd.x).abs();
            if dist < 220.0 * tk.k.max(0.5) && self.touch.cur_speed > 900.0 && closing {
                self.touch.last_bolt = tk.now;
                let away: i8 = if tk.cx < self.bd.x { 1 } else { -1 };
                self.mind.act = Act::Zoomies(away);
                self.mind.act_until = tk.now + secs(&mut self.rng, 1.2, 2.2);
                self.ctl.lock().unwrap().note(format!("bolted from the cursor (bond {:.2})", self.bond.v));
            }
        }

        // --- petting: strokes of the cursor over her (no button) ---
        if self.touch.prev_over && over && tk.grounded && !matches!(self.bd.mode, Mode::Held(..)) && !tk.lbutton {
            self.touch.stroke += (tk.cx - self.touch.last_cur.0).abs() + (tk.cy - self.touch.last_cur.1).abs() * 0.5;
        }
        self.touch.stroke *= (1.0 - tk.dt * 0.9).max(0.0);
        self.touch.last_cur = (tk.cx, tk.cy);
        self.touch.prev_over = over;
        if self.touch.stroke > 450.0 {
            self.touch.last_stroke = tk.now;
            if !self.touch.purring {
                self.touch.purring = true;
                // a pet is worth most once in a while (strokes in a row don't stack up endlessly)
                let fresh = tk.now - self.touch.last_pet_bonus > Duration::from_secs(60);
                self.bond.add(if fresh { crate::bond::PET } else { crate::bond::PET_AGAIN });
                if fresh {
                    self.touch.last_pet_bonus = tk.now;
                }
                self.ctl.lock().unwrap().note(format!("petted (bond {:.2})", self.bond.v));
            }
            if !matches!(self.mind.act, Act::Sleep | Act::Carry | Act::Fetch) && matches!(self.bd.mode, Mode::Ground(_)) {
                self.mind.act = Act::Attend;
                self.mind.act_until = tk.now + Duration::from_secs(3);
            }
            self.mind.needs.boredom = 0.0;
        }
        // petting ends 1.5 s after the last stroke (purring = being petted, with or without Sound)
        if self.touch.purring && tk.now - self.touch.last_stroke > Duration::from_millis(1500) {
            self.touch.purring = false;
        }
        // her meow: rare, and only when nothing else is going on (sound.rs checks quiet apps + output level)
        if tk.want_meow && tk.sound_on && !self.tape.replaying() {
            if tk.now - self.mind.last_meow >= MEOW_GAP {
                self.mind.last_meow = tk.now;
                let log = self.ctl.clone();
                crate::sound::meow_if_calm(tk.quiet_apps.clone(), move |why| log.lock().unwrap().note(why));
            } else {
                self.ctl.lock().unwrap().note(format!("meow skipped: last one {} min ago", (tk.now - self.mind.last_meow).as_secs() / 60));
            }
        }

        if !self.tape.replaying() && tk.now - self.scr.last_status > Duration::from_millis(250) {
            self.scr.last_status = tk.now;
            let status = json!({
                "act": self.mind.act.name(),
                "mode": self.bd.mode.name(),
                "anim": self.sheet.anims[tk.sel.anim].name,
                "mirror": tk.sel.mirror,
                "sprite": self.sheet.kind,
                "skin": self.sheet.skin,
                "skins": crate::render::SKINS.iter().map(|s| json!({"id": s.id, "label": s.label})).collect::<Vec<_>>(),
                "needs": self.mind.needs,
                "mood": tk.mood,
                "quiet": self.sense.quiet,
                "fullscreenHidden": self.scr.shown != !tk.hidden,
                "hidden": tk.hidden,
                "size": tk.size,
                "night": tk.night,
                "userIdleS": self.sense.user_idle.as_secs(),
                "position": {"x": self.bd.x.round(), "y": self.bd.y.round(), "monitor": {"left": self.bd.mon.work.left, "right": self.bd.mon.work.right, "primary": self.bd.mon.primary}},
                "window": {"x": self.scr.placed.0, "y": self.scr.placed.1, "w": self.scr.placed.2, "h": self.scr.placed.3},
                "interactive": self.scr.interactive,
                "sound": tk.sound_on,
                "memoryMB": tk.webview_mb,
                "purring": self.touch.purring,
                "halo": {"visible": self.halo.visible(), "level": (self.halo.level() * 100.0).round() / 100.0},
                "card": {"visible": self.card.visible(), "pinned": self.card.pinned(), "rect": self.card.rect(), "tray": self.ctl.lock().unwrap().tray.rect.map(|r| [r.left, r.top, r.right, r.bottom])},
                "stroke": self.touch.stroke.round(),
                "over": self.touch.prev_over,
                "cursor": [tk.cx, tk.cy],
                "mischief": tk.mischief,
                "nextMischiefInS": self.job.next_carry.saturating_duration_since(tk.now).as_secs(),
                "carry": self.job.carry.as_ref().map(|c| json!({"name": c.name, "dx": c.d as f64 * c.dist})),
                "windows": self.world.wins.iter().take(12).enumerate().map(|(i, w)| json!({
                    "rect": [w.rect.left, w.rect.top, w.rect.right, w.rect.bottom],
                    "max": w.maximized,
                    "standableMid": self.world.standable(i, (w.rect.left + w.rect.right) as f64 / 2.0),
                })).collect::<Vec<_>>(),
            });
            let mut c = self.ctl.lock().unwrap();
            let mut status = status;
            status["recent"] = json!(c.log.iter().collect::<Vec<_>>());
            status["bed"] = self.bed.status();
            status["bed"]["on"] = json!(tk.bed_on);
            status["bond"] = json!({"value": (self.bond.v * 1000.0).round() / 1000.0, "level": self.bond.level(), "wary": self.bond.wary(), "close": self.bond.close(), "napByCursor": self.mind.nap_by_cursor});
            status["warm"] = json!({
                "gpuCounters": self.sense.gpu_ok,
                "hot": self.sense.hot.as_ref().map(|(wi, l)| json!({"pid": wi.pid, "exe": win::pid_exe(wi.pid), "load20s": (l * 10.0).round() / 10.0})),
                "target": self.job.warm.map(|(hw, pid)| json!({"hwnd": hw, "pid": pid, "x": self.job.warm_tx.round()})),
                "nap": self.job.warm_nap,
                "asked": self.job.warm_force,
                "offBelow": self.job.warm_off,
                "coolingS": self.job.cool_since.map(|t| (tk.now - t).as_secs()),
                "nextTripInS": self.job.next_warm.saturating_duration_since(tk.now).as_secs(),
                "on": WARM_ON,
                "top6s": self.sense.gpu_top.iter().map(|(p, l)| json!({"pid": p, "exe": win::pid_exe(*p), "load": (l * 10.0).round() / 10.0})).collect::<Vec<_>>(),
            });
            c.status = status;
        }

        // --- petting halo (PLAN #57): a pink glow breathes around her while she is petted ---
        let glow = self.scr.glow_test.is_some_and(|t| tk.now < t);
        if !glow {
            self.scr.glow_test = None;
        }
        self.halo.step(tk.now, tk.raw_dt.min(0.1), self.scr.shown && (self.touch.purring || glow), self.hwnd, self.overlay.buffer(), tk.size);

        // --- status card over the tray icon (PLAN #54) ---
        let (tray, card_pin) = self.tape.val("ctl_card", || {
            let mut c = self.ctl.lock().unwrap();
            (c.tray, c.card_pin.take())
        });
        if let Some((on, pos)) = card_pin {
            self.card.set_pin(on, pos, &self.world.mons);
        }
        let info = crate::card::Info {
            sheet: &self.sheet,
            anim: tk.sel.anim,
            frame: self.overlay.frame,
            mirror: tk.sel.mirror,
            activity: activity(self.mind.act, self.bd.mode, self.touch.purring, self.job.warm_nap, self.sheet.anims[tk.sel.anim].pose),
            asleep: self.mind.act == Act::Sleep,
            mood: tk.mood,
            needs: [self.mind.needs.sleepiness, self.mind.needs.boredom, self.mind.needs.playfulness],
            bond: self.bond.v,
            bond_level: self.bond.level(),
        };
        let was = self.card.visible();
        // a pinned card steps aside with her: hidden, or a fullscreen app on the card's screen
        let (crx, cry, crw, crh) = self.card.rect();
        let card_mon = self.world.mons.iter().find(|m| inside(&m.rect, (crx + crw / 2) as f64, (cry + crh / 2) as f64)).map(|m| m.handle);
        let hide_pinned = tk.hidden || (self.sense.fullscreen.is_some() && self.sense.fullscreen == card_mon);
        self.card.step(tk.now, tk.raw_dt.min(0.1), tk.cursor, tk.lbutton, hide_pinned, tray, &self.world.mons, &info);
        if self.card.visible() != was {
            self.ctl.lock().unwrap().note(format!("status card {}", if self.card.visible() { "shown" } else { "hidden" }));
        }

        // scenario replays (tape.rs) aim the cursor at her / her bed: where they are now
        self.tape.anchor("henry", self.scr.placed.0 as f64 + self.scr.placed.2 as f64 / 2.0, self.scr.placed.1 as f64 + self.scr.placed.3 as f64 * 0.55);
        self.tape.anchor("bed", self.bed.x, self.bed.y - 12.0 * tk.k);

        // --- replay trace (tape.rs): her whole state after this tick + the notes it wrote ---
        if let Some(tf) = self.replay_trace.as_mut() {
            use std::io::Write;
            let an = &self.sheet.anims[tk.sel.anim];
            let _ = writeln!(
                tf,
                "{}\t{}\t{mind_act:?}\t{bd_mode:?}\t{bd_x:?}\t{bd_y:?}\t{bd_vx:?}\t{bd_vy:?}\t{bd_face}\t{ps_pose:?}\t{}\t{}\t{}\t{scr_placed:?}\t{pin:?}\t{:?}\t{:?}\t{touch_purring}\t{touch_stroke:?}\t{:?}\t{:?}\t{:?}\t{:?}\t{:?}",
                self.tape.tick,
                (tk.now - self.t0).as_nanos(),
                an.name,
                self.overlay.frame,
                tk.sel.mirror,
                self.mind.needs,
                self.bond.v,
                (self.bed.x, self.bed.y, self.bed.mode),
                self.ps.trans.map(|(a, t)| (a, (t - self.t0).as_nanos())),
                self.ps.trans_next.front(),
                (self.job.warm, self.job.warm_nap, self.job.carry.as_ref().map(|c| (c.index, c.last_dx))),
                (self.mind.act_until - self.t0).as_nanos(), mind_act = self.mind.act, bd_mode = self.bd.mode, bd_x = self.bd.x, bd_y = self.bd.y, bd_vx = self.bd.vx, bd_vy = self.bd.vy, bd_face = self.bd.face, ps_pose = self.ps.pose, scr_placed = self.scr.placed, touch_purring = self.touch.purring, touch_stroke = self.touch.stroke,
            );
            let c = self.ctl.lock().unwrap();
            let fresh = (c.noted - self.noted_seen).min(c.log.len() as u64) as usize;
            for n in c.log.iter().skip(c.log.len() - fresh) {
                let n = n.split_once(' ').map_or(n.as_str(), |(_, rest)| rest);
                let _ = writeln!(tf, "  note: {n}");
            }
            self.noted_seen = c.noted;
        }
    }
}
