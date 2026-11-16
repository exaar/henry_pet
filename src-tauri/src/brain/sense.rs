//! Phase 1 of a tick: inputs (API requests, the desktop, the cursor), her size, the bed in the user's hand, needs.
use super::*;
use crate::win::{self};
use std::time::{Duration, Instant};

impl Brain {
    /// Inputs of this tick: API requests, the desktop, the cursor; her size; the bed in the user's hand; needs.
    pub(super) fn sense(&mut self, now: Instant) -> Tick {
        self.overlay.pump();
        let raw_dt = (now - self.last).as_secs_f64();
        if raw_dt > 0.15 {
            self.ctl.lock().unwrap().note(format!("stall {:.0} ms in {}", raw_dt * 1000.0, self.bd.mode.name()));
        }
        let dt = raw_dt.min(0.05);
        self.last = now;
        let icons_path = self.ctl.lock().unwrap().icons_path.clone();
        let (hidden, size, quiet_apps, mood, beat, cmd, sound_on, mischief, webview_mb, skin) = self.tape.val("ctl", || {
            let mut c = self.ctl.lock().unwrap();
            let mood = c.mood();
            let beat = c.beats.pop_front();
            let cmd = c.commands.pop_front();
            let skin = if c.skin != self.sheet.skin { Some(c.skin.clone()) } else { None };
            (c.hidden, c.size, c.quiet_apps.clone(), mood, beat, cmd, c.sound, c.mischief, c.webview_mb, skin)
        });
        // tray "Skin": swap the sheet live (an unknown id resolves to the first skin; store what we got)
        if let Some(id) = skin {
            self.sheet = crate::render::Sheet::load(&id);
            self.sprite = self.sheet.info.clone();
            self.has_takeoff = self.sheet.has("jump");
            self.can_leap = leap_art(&self.sheet);
            self.picker = crate::render::Picker::new();
            // indices into the old sheet: a running transition would index past the new one (panic)
            self.ps.drop_clips();
            self.bd.swing = None;
            self.overlay.invalidate();
            self.ps.epoch += 1;
            let mut c = self.ctl.lock().unwrap();
            c.note(format!("skin {} ({})", self.sheet.skin, self.sheet.kind));
            c.skin = self.sheet.skin.to_string();
        }
        let (bed_on, bed_cmd, bond_set) = self.tape.val("ctl_bed", || {
            let mut c = self.ctl.lock().unwrap();
            (c.bed, c.bed_cmds.pop_front(), c.bond_set.take())
        });
        if let Some(ns) = self.tape.val("needs_set", || self.ctl.lock().unwrap().needs_set.take()) {
            let slots = [&mut self.mind.needs.sleepiness, &mut self.mind.needs.boredom, &mut self.mind.needs.playfulness];
            for (slot, v) in slots.into_iter().zip(ns) {
                if let Some(v) = v {
                    *slot = v.clamp(0.0, 1.0);
                }
            }
        }
        if let Some(v) = bond_set {
            self.bond.set(v);
            self.bond.save();
            self.ctl.lock().unwrap().note(format!("bond set to {:.2} ({})", self.bond.v, self.bond.level()));
        }
        self.bond.tick(now);
        let was_in_bed = self.bd.mode == Mode::Ground(Surface::Bed);
        let cursor = self.tape.val("cursor", win::cursor);
        let (cx, cy) = (cursor.0 as f64, cursor.1 as f64);
        // Grab = left button goes down while the cursor is over her (detected here, not via the webview).
        let lbutton = self.tape.val("lbutton", win::left_button_down);
        let grab = lbutton && !self.touch.prev_lbutton && self.touch.prev_over && self.scr.shown;
        let grab_bed = lbutton && !self.touch.prev_lbutton && self.touch.prev_over_bed && !self.touch.prev_over && bed_on;
        self.touch.prev_lbutton = lbutton;

        if now - self.sense.last_env > Duration::from_millis(400) {
            self.sense.last_env = now;
            self.world.mons = self.tape.val("mons", win::monitors);
            let (fg_full, fg_exe) = self.tape.val("fg", || {
                let fg = win::foreground(self.hwnd);
                (fg.fullscreen_monitor, fg.exe)
            });
            self.sense.fullscreen = fg_full;
            self.sense.quiet = fg_exe.is_some_and(|e| quiet_apps.iter().any(|q| q.eq_ignore_ascii_case(&e)));
            self.sense.user_idle = self.tape.val("idle", win::user_idle);
            self.sense.hour = self.tape.val("hour", win::local_hour);
            let warm_pid = self.job.warm.map(|(_, pid)| pid);
            let (ok, h, top, warm_load) = self.tape.val("gpu", || {
                let g = self.gpu.as_ref().expect("live gpu").lock().unwrap();
                let mut top: Vec<(u32, f64)> = g.hist.keys().map(|&p| (p, g.mean(p, 3))).filter(|(_, l)| *l >= 1.0).collect();
                top.sort_by(|a, b| b.1.total_cmp(&a.1));
                top.truncate(4);
                (g.ok, hottest(&self.world, &g, 10), top, warm_pid.map(|pid| g.mean(pid, 3)))
            });
            self.sense.gpu_ok = ok;
            self.sense.hot = h;
            self.sense.gpu_top = top;
            // napping on the warm window: it cooled down (render done) -> she wakes up and stretches
            if let Some(load) = warm_load.filter(|_| self.job.warm.is_some()) {
                let cooled = load < self.job.warm_off;
                self.job.cool_since = if cooled { Some(self.job.cool_since.unwrap_or(now)) } else { None };
            }
        }
        if now - self.sense.last_wins > Duration::from_millis(100) {
            self.sense.last_wins = now;
            self.world.wins = self.tape.val("wins", || win::windows(self.hwnd));
        }
        // the windows she and her bed stand on: fresh position every tick, so they keep up with a drag
        for hw in [
            if let Mode::Ground(Surface::Window(h)) = self.bd.mode { Some(h) } else { None },
            if let BedMode::Ground(Surface::Window(h)) = self.bed.mode { Some(h) } else { None },
        ]
        .into_iter()
        .flatten()
        {
            match self.tape.keyed("frame", hw, || self.world.wins.iter().find(|w| w.hwnd == hw).map(|w| (w.rect, w.maximized)), || win::frame_rect(hw)) {
                Some((r, max)) => {
                    if let Some(w) = self.world.wins.iter_mut().find(|w| w.hwnd == hw) {
                        w.rect = r;
                        w.maximized = max;
                    }
                }
                None => self.world.wins.retain(|w| w.hwnd != hw), // gone: she falls this tick
            }
        }

        // tray Size = cell WIDTH on screen; the cell may be taller than wide (tall poses: jump take-off)
        let k = size as f64 / self.sprite.cell_w;
        let w = (self.sprite.cell_w * k).round() as i32;
        let h = (self.sprite.cell_h * k).round() as i32;
        self.world.h = h as f64;
        let half_body = w as f64 * 0.42;

        // --- the bed: the user grabs it / the API drops it somewhere ---
        if grab_bed && !matches!(self.bed.mode, BedMode::Held(_)) {
            if matches!(self.bed.mode, BedMode::Carried) {
                self.mind.act = Act::Look; // it was taken out of her mouth
                self.mind.act_until = now + Duration::from_secs(2);
            }
            self.bed.grab(cx, cy, k);
            self.ctl.lock().unwrap().note("bed grabbed".into());
        }
        if let Some(BedCmd::Place(px)) = bed_cmd {
            let px = px.unwrap_or(self.bd.x + self.bd.face as f64 * 260.0);
            if let Some(m) = monitor_at(&self.world.mons, px, self.bd.y - 2.0).cloned() {
                let half = crate::bed::BW / 2.0 * k;
                self.bed.x = px.clamp(m.work.left as f64 + half, m.work.right as f64 - half);
                self.bed.y = m.work.top as f64 + crate::bed::DH as f64 * k + 10.0;
                self.bed.drop_it(0.0, 0.0);
                self.ctl.lock().unwrap().note(format!("bed placed at x={:.0}", self.bed.x));
            }
        }
        // in the bed only while it lies somewhere: grabbed, thrown, put away -> she tumbles out
        if self.bd.mode == Mode::Ground(Surface::Bed) && !(bed_on && self.bed.grounded()) {
            self.bd.mode = Mode::Air;
            (self.bd.vx, self.bd.vy) = (0.0, -300.0);
            if self.mind.act == Act::Sleep {
                self.mind.act = Act::Idle;
                self.mind.act_until = now + Duration::from_secs(2);
            }
        }
        let bed_mon = monitor_at(&self.world.mons, self.bed.x, self.bed.y - 2.0).map(|m| m.handle);
        let bed_ready = bed_on && self.bed.grounded() && (self.sense.fullscreen.is_none() || bed_mon != self.sense.fullscreen);
        let night = !(7..23).contains(&self.sense.hour);
        let away = self.sense.user_idle > Duration::from_secs(300);
        let user_active = self.sense.user_idle < Duration::from_secs(60);
        let asleep = self.mind.act == Act::Sleep;
        let grounded = matches!(self.bd.mode, Mode::Ground(_));
        if let Some(m) = monitor_at(&self.world.mons, self.bd.x, self.bd.y - 2.0) {
            self.bd.mon = m.clone();
        }

        // --- needs ---
        let t = dt * self.time_scale;
        if asleep && grounded {
            self.mind.needs.sleepiness = (self.mind.needs.sleepiness - t / (25.0 * 60.0)).max(0.0);
        } else {
            let rate = if night { 2.0 } else { 1.0 } * if away { 1.8 } else { 1.0 };
            self.mind.needs.sleepiness = (self.mind.needs.sleepiness + t * rate / (45.0 * 60.0)).min(1.0);
            if user_active {
                // a close cat wants the user more often (x0.5 .. x1.5)
                self.mind.needs.boredom = (self.mind.needs.boredom + t * (0.5 + self.bond.v) / (25.0 * 60.0)).min(1.0);
            }
            self.mind.needs.playfulness = (self.mind.needs.playfulness + t / (30.0 * 60.0)).min(1.0);
        }
        // company: the user at the PC while she is out warms her up (PLAN #64)
        if user_active && !hidden {
            self.bond.company(t);
        }

        Tick { now, raw_dt, dt, icons_path, hidden, size, quiet_apps, mood, beat, cmd, sound_on, mischief, webview_mb, bed_on, was_in_bed, want_meow: false, greet_cmd: false, cursor, cx, cy, lbutton, grab, k, w, h, half_body, bed_ready, night, away, user_active, grounded, x_tick0: 0.0, speed: 0.0, gap: 0.0, name: "", dir: 1, fb_ms: None, still: false, sel: crate::render::Pick { anim: 0, mirror: false, fallback: false }, cycle: 0, rotated: false, rotate: 0, lift: 0.0 }
    }
}
