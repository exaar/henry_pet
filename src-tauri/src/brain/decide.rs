//! Phase 2: what she does next - grabs, API commands, Hermes moods, the user away/back, quiet apps, errands, choice.
use super::*;
use crate::win::{self};
use rand::Rng;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

impl Brain {
    /// What she does next: grabs, API commands, Hermes moods, the user away/back, quiet apps, errands, choice.
    pub(super) fn decide(&mut self, tk: &mut Tick) {
        // --- grabbed: the webview reported a press on her ---
        if tk.grab && !matches!(self.bd.mode, Mode::Held(..)) {
            // picked up: just being carried (no bond cost); yanked out of sleep: a little (PLAN #64)
            if self.mind.act == Act::Sleep {
                self.bond.add(crate::bond::WOKEN);
            }
            self.touch.tossed = false;
            if self.bd.mode == Mode::Ground(Surface::Bed) {
                self.bed.jelly.kick(0.0, 0.6, -50.0, -1.5);
            }
            self.bd.mode = Mode::Held(self.bd.x - tk.cx, self.bd.y - tk.cy);
            // where her scruff would be if the held frame were drawn where she is now -> glides to the cursor
            self.bd.swing = self.sheet.anims.iter().find(|a| crate::render::in_group(&a.name, "held")).and_then(|a| a.pivot.map(|pv| (a, pv))).map(|(a, (pvx, pvy))| {
                let p = (self.bd.x - tk.w as f64 / 2.0 + pvx * tk.k, self.bd.y - self.sprite.foot_y * tk.k + pvy * tk.k);
                crate::slinky::Slinky::new(p, (pvx, pvy), self.sheet.art_bottom(a), tk.k)
            });
            self.bd.samples.clear();
            self.job.carry = None;
            self.bd.perch_target = None;
            self.mind.act = Act::Idle;
            self.bd.held_since = tk.now;
            let (btn, ms) = self.tape.val_or("grabinfo", || (true, false), || (win::left_button_down(), win::in_move_size(self.hwnd)));
            self.ctl.lock().unwrap().note(format!("grabbed (button {btn}, movesize {ms})"));
        }

        // --- commands from the API (explicit asks win; they are the user's) ---
        // glow test: only the halo, whatever she is doing goes on
        let cmd = if matches!(tk.cmd, Some(Command::Glow)) {
            self.scr.glow_test = Some(tk.now + Duration::from_secs(3));
            None
        } else {
            tk.cmd
        };
        if let Some(c) = cmd {
            let mut next = None;
            match c {
                Command::Sleep => next = Some((Act::Sleep, Duration::from_secs(600))),
                Command::Wake => {
                    self.mind.needs.sleepiness = self.mind.needs.sleepiness.min(0.3);
                    next = Some((Act::Beat("wave"), beat_dur(&self.sheet, "wave", 2200)));
                }
                Command::Walk => next = Some((Act::Walk(if self.rng.gen_bool(0.5) { 1 } else { -1 }), Duration::from_secs(8))),
                Command::Come => next = Some((Act::Come, Duration::from_secs(40))),
                Command::Greet => {
                    tk.greet_cmd = true;
                    next = Some((Act::Come, Duration::from_secs(40)));
                }
                Command::Wave => next = Some((Act::Beat("wave"), beat_dur(&self.sheet, "wave", 2200))),
                Command::Groom => next = Some((Act::Beat("groom"), beat_dur(&self.sheet, "groom", 4000))),
                Command::Stretch => next = Some((Act::Beat("stretch"), beat_dur(&self.sheet, "stretch", 1400))),
                Command::Zoomies => next = Some((Act::Zoomies(if self.rng.gen_bool(0.5) { 1 } else { -1 }), Duration::from_secs(6))),
                Command::Idle => next = Some((Act::Idle, Duration::from_secs(8))),
                Command::Glow => {} // handled above: it must not interrupt what she is doing
                Command::Jump => {
                    if tk.grounded {
                        if let Some((tx, ty)) = self.world.jump_target(&mut self.rng, self.bd.x, self.bd.y) {
                            if self.has_takeoff {
                                self.bd.takeoff = Some((tx, ty));
                                self.bd.face = if tx < self.bd.x { -1 } else { 1 }; // crouch facing where she will fly
                                next = Some((Act::Beat("jump"), beat_dur(&self.sheet, "jump", 300)));
                            } else {
                                (self.bd.vx, self.bd.vy) = ballistic(self.bd.x, self.bd.y, tx, ty);
                                self.bd.mode = Mode::Air;
                            }
                        }
                    }
                }
                Command::Climb => {
                    if let Some(d) = outer_wall(&self.world.mons, &self.bd.mon, self.bd.x) {
                        if tk.grounded {
                            self.bd.mode = Mode::Ground(Surface::Floor);
                            self.bd.y = self.bd.mon.work.bottom as f64;
                        }
                        next = Some((Act::ToWall(d), Duration::from_secs(60)));
                    }
                }
                Command::Bed => {
                    if tk.bed_on {
                        self.job.bed_deadline = tk.now + Duration::from_secs(60);
                        next = Some((Act::ToBed, Duration::from_secs(60)));
                    }
                }
                Command::Movebed => {
                    if tk.bed_ready && matches!(self.bd.mode, Mode::Ground(Surface::Floor)) {
                        match drag_dir(&self.world, &self.bed, tk.k, tk.half_body) {
                            Some(d) => next = Some((Act::FetchBed(d), Duration::from_secs(40))),
                            None => self.ctl.lock().unwrap().note("movebed: no room around the bed".into()),
                        }
                    }
                }
                Command::Warm => {
                    let found = self.tape.val_or("warm_cmd", || self.sense.hot.clone(), || hottest(&self.world, &self.gpu.as_ref().expect("live gpu").lock().unwrap(), 3));
                    match found.filter(|(_, l)| *l >= 1.0) {
                        Some((wi, load)) if tk.grounded => {
                            self.job.warm = Some((wi.hwnd, wi.pid));
                            self.job.warm_tx = warm_x(&self.world, &wi, tk.half_body);
                            self.job.warm_off = WARM_OFF.min(load * 0.4);
                            self.job.warm_force = true;
                            self.job.warm_deadline = tk.now + Duration::from_secs(90);
                            next = Some((Act::ToWarm, Duration::from_secs(90)));
                            let exe = self.tape.keyed("exe", wi.pid, || None, || win::pid_exe(wi.pid));
                            self.ctl.lock().unwrap().note(format!("warm (asked): pid {} {:?} at {load:.0}%", wi.pid, exe));
                        }
                        _ => {
                            self.ctl.lock().unwrap().note("warm: no windowed process on the GPU".into());
                        }
                    }
                }
                Command::Meow => {
                    let ok = tk.sound_on && self.tape.val_or("meow", || false, crate::sound::meow); // asked: plays now, no calm check
                    self.ctl.lock().unwrap().note(format!("meow (sound {}, played {ok})", tk.sound_on));
                }
                Command::Carry => {
                    self.job.carry_asap = true;
                    if !matches!(self.bd.mode, Mode::Ground(Surface::Floor)) {
                        self.ctl.lock().unwrap().note("carry: getting down first".into());
                        if let Mode::Ground(Surface::Window(_)) = self.bd.mode {
                            // off a window: walk to its nearer edge and hop off there
                            let d = self.bd.on_rect.map_or(1, |r| if self.bd.x - (r.left as f64) < r.right as f64 - self.bd.x { -1 } else { 1 });
                            self.job.get_down = Some(d);
                            next = Some((Act::Walk(d), Duration::from_secs(30)));
                        } else if let Mode::Ground(_) = self.bd.mode {
                            self.bd.vx = if self.rng.gen_bool(0.5) { 250.0 } else { -250.0 };
                            self.bd.vy = -450.0;
                            self.bd.mode = Mode::Air;
                        }
                    }
                }
                Command::Down => match self.bd.mode {
                    Mode::Ground(Surface::Window(_)) => {
                        // walk to the nearer edge and hop off there (from the middle she landed back on it)
                        let d = self.bd.on_rect.map_or(1, |r| if self.bd.x - (r.left as f64) < r.right as f64 - self.bd.x { -1 } else { 1 });
                        self.job.get_down = Some(d);
                        next = Some((Act::Walk(d), Duration::from_secs(30)));
                    }
                    Mode::Ground(Surface::Perch(_)) | Mode::Ground(Surface::Bed) | Mode::Cling(_) | Mode::Climb(..) => {
                        if let Mode::Cling(d) | Mode::Climb(d, _) = self.bd.mode {
                            self.bd.y += tk.w as f64 / 2.0 + (self.sprite.cell_h * tk.k - self.sprite.foot_y * tk.k);
                            self.bd.x -= d as f64 * tk.w as f64 * 0.6;
                        }
                        self.bd.vx = if self.rng.gen_bool(0.5) { 250.0 } else { -250.0 };
                        self.bd.vy = -450.0;
                        self.bd.mode = Mode::Air;
                    }
                    _ => {}
                },
            }
            if let Some((a, d)) = next {
                self.mind.act = a;
                self.mind.act_until = tk.now + d;
            }
        }

        // --- Hermes beats and moods: they colour, never command ---
        if let Some(b) = tk.beat {
            if self.mind.act == Act::Sleep || !tk.grounded {
                self.ps.twitch_until = Some(tk.now + Duration::from_millis(400));
            } else if !self.sense.quiet {
                let name = match b {
                    Mood::Failed => "failed",
                    Mood::Jump => "hop",
                    _ => "wave",
                };
                self.mind.act = Act::Beat(name);
                self.mind.act_until = tk.now + beat_dur(&self.sheet, name, if b == Mood::Failed { 1600 } else { 2200 });
            }
        }
        if tk.mood != self.mind.last_mood {
            if self.mind.act == Act::Sleep {
                self.ps.twitch_until = Some(tk.now + Duration::from_millis(400));
            }
            if tk.mood != Mood::Waiting {
                self.mind.attended_waiting = false;
            }
            self.mind.last_mood = tk.mood;
        }

        // --- The user came back after being away: if not too sleepy, wake and greet; after a long absence
        // (15 min+) she meows when she reaches him (PLAN #55) ---
        if tk.away {
            self.mind.was_away = true;
            self.mind.away_for = self.mind.away_for.max(self.sense.user_idle);
        } else if self.mind.was_away && tk.user_active {
            self.mind.was_away = false;
            let long = self.mind.away_for >= Duration::from_secs(15 * 60);
            self.mind.away_for = Duration::ZERO;
            let awake_free = tk.grounded && matches!(self.mind.act, Act::Idle | Act::Walk(_) | Act::Look | Act::Rest);
            let can_wake = self.mind.act == Act::Sleep && self.mind.needs.sleepiness < 0.5 && !self.job.warm_nap;
            if !self.sense.quiet && (can_wake || (long && awake_free)) {
                self.mind.act = Act::Come;
                self.mind.act_until = tk.now + Duration::from_secs(40);
                self.mind.needs.boredom = 0.0;
                self.mind.greet = long;
            }
        }
        if tk.greet_cmd {
            self.mind.greet = true;
            self.mind.last_meow = tk.now - MEOW_GAP; // the test ignores the 2 h gap, not the calm check
        }
        if self.mind.act != Act::Come {
            self.mind.greet = false; // the greeting trip ended some other way: no meow later
        }

        // --- quiet app in front: get down, corner and stay ---
        // (a warm trip may go on while the user leaves it alone: a render they are waiting for, PLAN #52)
        if self.sense.quiet && self.mind.act == Act::ToWarm && self.sense.user_idle < Duration::from_secs(60) {
            self.mind.act = Act::Idle;
            self.job.warm = None;
            self.job.warm_jump = false;
            self.job.next_warm = tk.now + Duration::from_secs(120);
        }
        if self.sense.quiet {
            if self.job.carry.is_some() {
                self.job.carry = None;
                self.bd.perch_target = None;
            }
            match self.bd.mode {
                Mode::Ground(Surface::Perch(_)) => {
                    (self.bd.vx, self.bd.vy) = (0.0, 0.0);
                    self.bd.mode = Mode::Air;
                }
                Mode::Climb(d, _) | Mode::Cling(d) => {
                    self.bd.y += tk.w as f64 / 2.0 + (self.sprite.cell_h * tk.k - self.sprite.foot_y * tk.k);
                    self.bd.x -= d as f64 * tk.w as f64 * 0.6;
                    (self.bd.vx, self.bd.vy) = (0.0, 0.0);
                    self.bd.mode = Mode::Air;
                }
                Mode::Ground(Surface::Window(_)) if !matches!(self.mind.act, Act::ToCorner(_) | Act::Sleep | Act::ToWarm) => {
                    let to_left = self.bd.x - (self.bd.mon.work.left as f64) < (self.bd.mon.work.right as f64) - self.bd.x;
                    self.mind.act = Act::ToCorner(if to_left { -1 } else { 1 });
                }
                Mode::Ground(Surface::Floor) if !matches!(self.mind.act, Act::ToCorner(_) | Act::Rest | Act::Sleep | Act::ToWarm) => {
                    let to_left = self.bd.x - (self.bd.mon.work.left as f64) < (self.bd.mon.work.right as f64) - self.bd.x;
                    self.mind.act = Act::ToCorner(if to_left { -1 } else { 1 });
                }
                _ => {}
            }
        } else if matches!(self.mind.act, Act::ToCorner(_) | Act::Rest) {
            self.mind.act = Act::Idle;
            self.mind.act_until = tk.now + Duration::from_secs(1);
        }

        // --- warm spot (PLAN #52) ---
        // napping on it: the GPU cooled (render done) -> up with a stretch; woken / grabbed / fell -> trip over
        if self.job.warm_nap {
            if self.mind.act != Act::Sleep {
                self.job.warm_nap = false;
                self.job.warm = None;
                self.job.next_warm = tk.now + Duration::from_secs(600);
                self.ctl.lock().unwrap().note("warm nap disturbed".into());
            } else if self.job.cool_since.is_some_and(|t| tk.now - t > Duration::from_secs(4)) {
                self.job.warm_nap = false;
                self.job.warm = None;
                self.job.cool_since = None;
                self.job.next_warm = tk.now + Duration::from_secs(180);
                if self.sheet.has("stretch") {
                    self.mind.act = Act::Beat("stretch");
                    self.mind.act_until = tk.now + beat_dur(&self.sheet, "stretch", 1400);
                } else {
                    self.mind.act = Act::Look;
                    self.mind.act_until = tk.now + Duration::from_secs(3);
                }
                self.ctl.lock().unwrap().note("warm spot cooled -> awake".into());
            }
        }
        // something got hot: she goes over (not while busy, sleeping, or the user working in a quiet app)
        if self.job.warm.is_none()
            && tk.now >= self.job.next_warm
            && tk.grounded
            && self.bd.takeoff.is_none()
            && self.job.carry.is_none()
            && !self.touch.purring
            && tk.mood != Mood::Waiting
            && (!self.sense.quiet || self.sense.user_idle > Duration::from_secs(60))
            && matches!(self.mind.act, Act::Idle | Act::Walk(_) | Act::Look | Act::Rest | Act::ToCorner(_) | Act::Zoomies(_))
            && !matches!(self.bd.mode, Mode::Ground(Surface::Perch(_)))
        {
            if let Some((wi, load)) = self.sense.hot.as_ref().filter(|(_, l)| *l >= WARM_ON) {
                self.job.warm = Some((wi.hwnd, wi.pid));
                self.job.warm_tx = warm_x(&self.world, wi, tk.half_body);
                self.job.warm_off = WARM_OFF;
                self.job.warm_force = false;
                self.job.warm_deadline = tk.now + Duration::from_secs(90);
                self.mind.act = Act::ToWarm;
                self.mind.act_until = self.job.warm_deadline;
                let exe = self.tape.keyed("exe", wi.pid, || None, || win::pid_exe(wi.pid));
                self.ctl.lock().unwrap().note(format!("warm spot: pid {} {:?} at {load:.0}%", wi.pid, exe));
            }
        }
        // on the way: on the warm window -> nap; under it -> hop up if it can hold her, else nap on the floor
        if self.mind.act == Act::ToWarm && matches!(self.bd.mode, Mode::Ground(_)) && self.bd.takeoff.is_none() {
            let target = self.job.warm.and_then(|(hw, _)| self.world.win(hw).map(|(i, wi)| (i, wi.clone())));
            let give_up = |why: &str, ctl: &Arc<Mutex<Control>>| ctl.lock().unwrap().note(format!("warm trip off: {why}"));
            match target {
                None => {
                    give_up("window gone", &self.ctl);
                    self.job.warm = None;
                    self.mind.act = Act::Idle;
                    self.mind.act_until = tk.now + Duration::from_secs(2);
                    self.job.next_warm = tk.now + Duration::from_secs(120);
                }
                Some(_) if tk.now >= self.mind.act_until => {
                    give_up("took too long", &self.ctl);
                    self.job.warm = None;
                    self.mind.act = Act::Idle;
                    self.mind.act_until = tk.now + Duration::from_secs(2);
                    self.job.next_warm = tk.now + Duration::from_secs(300);
                }
                Some((i, wi)) => {
                    self.job.warm_tx = warm_x(&self.world, &wi, tk.half_body);
                    let nap = |act: &mut Act, act_until: &mut Instant, rng: &mut rand::rngs::StdRng| {
                        *act = Act::Sleep;
                        *act_until = tk.now + secs(rng, 240.0, 600.0);
                    };
                    if self.bd.mode == Mode::Ground(Surface::Window(wi.hwnd)) {
                        nap(&mut self.mind.act, &mut self.mind.act_until, &mut self.rng);
                        self.job.warm_nap = true;
                        self.job.cool_since = None;
                        self.ctl.lock().unwrap().note("napping on the warm window".into());
                    } else if self.bd.mode == Mode::Ground(Surface::Floor) && (self.job.warm_tx - self.bd.x).abs() < 150.0 * tk.k {
                        let top = wi.rect.top as f64;
                        let ok_top = (60.0..1350.0).contains(&(self.bd.y - top)) && self.world.standable(i, self.job.warm_tx);
                        if ok_top {
                            self.bd.face = if self.job.warm_tx < self.bd.x { -1 } else { 1 };
                            if self.has_takeoff {
                                self.bd.takeoff = Some((self.job.warm_tx, top));
                                self.job.warm_jump = true;
                                self.mind.act = Act::Beat("jump");
                                self.mind.act_until = tk.now + beat_dur(&self.sheet, "jump", 300);
                            } else {
                                (self.bd.vx, self.bd.vy) = ballistic(self.bd.x, self.bd.y, self.job.warm_tx, top);
                                self.bd.mode = Mode::Air;
                            }
                        } else if (self.job.warm_tx - self.bd.x).abs() < 4.0 {
                            // maximized / too high: right under its middle on the taskbar is warm enough
                            nap(&mut self.mind.act, &mut self.mind.act_until, &mut self.rng);
                            self.job.warm_nap = true;
                            self.job.cool_since = None;
                            self.ctl.lock().unwrap().note("napping under the warm window".into());
                        }
                    } else if matches!(self.bd.mode, Mode::Ground(Surface::Window(_))) && (self.job.warm_tx - self.bd.x).abs() < 150.0 * tk.k {
                        // on some other window right above the spot: get down first - off its nearer edge
                        // (hopping in place landed her back on it, over and over)
                        if self.job.get_down.is_none() {
                            self.job.get_down = Some(self.bd.on_rect.map_or(1, |r| if self.bd.x - (r.left as f64) < r.right as f64 - self.bd.x { -1 } else { 1 }));
                        }
                    }
                }
            }
        }

        // --- sleeping: wake when rested (at night with the user away she sleeps on) ---
        if self.mind.act == Act::Sleep && tk.now >= self.mind.act_until {
            let keep = self.mind.needs.sleepiness > 0.1 || (tk.night && tk.away) || self.job.warm_nap;
            if keep {
                self.mind.act_until = tk.now + Duration::from_secs(30);
            } else {
                self.mind.needs.playfulness = (self.mind.needs.playfulness + 0.3).min(1.0);
                if self.sheet.has("stretch") {
                    self.mind.act = Act::Beat("stretch");
                    self.mind.act_until = tk.now + beat_dur(&self.sheet, "stretch", 1400);
                } else {
                    self.mind.act = Act::Look;
                    self.mind.act_until = tk.now + Duration::from_secs(3);
                }
            }
        }

        // --- take-off crouch done: launch (recomputed from where she is now) ---
        if let Some((tx, ty)) = self.bd.takeoff {
            if !tk.grounded || (self.sense.quiet && !self.job.warm_jump) || self.mind.act != Act::Beat("jump") {
                self.bd.takeoff = None;
                self.job.bed_jump = false;
                self.job.warm_jump = false;
            } else if tk.now >= self.mind.act_until {
                // into the bed: aim where it is now (it may have been moved during the crouch)
                let (tx, ty) = if self.job.bed_jump && tk.bed_ready { self.bed.foot(tk.k, self.bd.face) } else { (tx, ty) };
                (self.bd.vx, self.bd.vy) = ballistic(self.bd.x, self.bd.y, tx, ty);
                self.bd.mode = Mode::Air;
                self.bd.takeoff = None;
                if self.job.bed_jump {
                    self.mind.act = Act::ToBed;
                    self.mind.act_until = self.job.bed_deadline;
                } else if self.job.warm_jump {
                    self.job.warm_jump = false;
                    self.mind.act = Act::ToWarm;
                    self.mind.act_until = self.job.warm_deadline;
                } else {
                    self.mind.act = Act::Idle;
                    self.mind.act_until = tk.now + secs(&mut self.rng, 2.0, 5.0);
                }
            }
        }

        // --- going to bed: hop in when close enough, else the body below walks her there ---
        // (mode, not the frame-start `grounded`: a launch earlier this frame already put her in the air)
        if self.mind.act == Act::ToBed && matches!(self.bd.mode, Mode::Ground(_)) && self.bd.mode != Mode::Ground(Surface::Bed) && self.bd.takeoff.is_none() {
            if !tk.bed_ready || tk.now >= self.job.bed_deadline {
                // no bed to go to (put away, carried off, unreachable): she naps right here
                self.mind.act = Act::Sleep;
                self.mind.act_until = tk.now + secs(&mut self.rng, 240.0, 600.0);
            } else {
                let (tx, ty) = self.bed.foot(tk.k, self.bd.face);
                let gapx = tx - self.bd.x;
                if gapx.abs() < 150.0 * tk.k && (-1500.0..1350.0).contains(&(self.bd.y - ty)) {
                    if gapx.abs() > 2.0 {
                        self.bd.face = if gapx < 0.0 { -1 } else { 1 };
                    }
                    self.job.bed_jump = true;
                    if self.has_takeoff {
                        self.bd.takeoff = Some((tx, ty));
                        self.mind.act = Act::Beat("jump");
                        self.mind.act_until = tk.now + beat_dur(&self.sheet, "jump", 300);
                    } else {
                        (self.bd.vx, self.bd.vy) = ballistic(self.bd.x, self.bd.y, tx, ty);
                        self.bd.mode = Mode::Air;
                    }
                }
            }
        }
        // --- fetching the bed: stand at its near end (it will be in front of her), bite, lift ---
        if let Act::FetchBed(d) = self.mind.act {
            if matches!(self.bd.mode, Mode::Ground(_)) {
                if !(tk.bed_on && matches!(self.bed.mode, BedMode::Ground(Surface::Floor))) || self.bd.mode != Mode::Ground(Surface::Floor) || tk.now >= self.mind.act_until {
                    self.mind.act = Act::Idle;
                    self.mind.act_until = tk.now + Duration::from_secs(2);
                } else {
                    let (gx, _) = crate::bed::grip(-d);
                    let mx = mouth("walk").0 - self.sprite.cell_w / 2.0;
                    self.job.fetch_tx = self.bed.x + (gx - crate::bed::DW as f64 / 2.0) * tk.k - d as f64 * mx * tk.k;
                    if (self.job.fetch_tx - self.bd.x).abs() < 4.0 && (self.bed.y - self.bd.y).abs() < 3.0 {
                        self.bd.x = self.job.fetch_tx;
                        self.bd.face = d;
                        self.bed.pick_up(tk.k, -d);
                        self.mind.act = Act::DragBed(d);
                        self.mind.act_until = tk.now + secs(&mut self.rng, 4.0, 9.0);
                        self.ctl.lock().unwrap().note(format!("bed picked up, dragging {d:+}"));
                    }
                }
            }
        }

        // --- choose the next thing (only while standing) ---
        if tk.grounded
            && !self.sense.quiet
            && (tk.now >= self.mind.act_until || (self.job.carry_asap && matches!(self.bd.mode, Mode::Ground(Surface::Floor))))
            && !matches!(self.mind.act, Act::Sleep | Act::Come | Act::ToWall(_) | Act::Fetch | Act::Carry | Act::ToBed | Act::FetchBed(_) | Act::DragBed(_) | Act::ToWarm)
            && !matches!(self.bd.mode, Mode::Ground(Surface::Perch(_)))
        {
            let on_window = matches!(self.bd.mode, Mode::Ground(Surface::Window(_)));
            let dir_toward_center = || {
                let center = (self.bd.mon.work.left + self.bd.mon.work.right) as f64 / 2.0;
                if self.bd.x < center { 1i8 } else { -1i8 }
            };
            let mut next: Option<(Act, Duration)> = None;
            if self.job.carry_asap && matches!(self.bd.mode, Mode::Ground(Surface::Floor)) {
                self.job.carry_asap = false;
                match plan_carry(&self.world, &mut self.tape, &mut self.rng, self.bd.y, tk.half_body) {
                    Ok(p) => {
                        self.ctl.lock().unwrap().note(format!("carry plan: '{}' {:+} px", p.name, p.d as f64 * p.dist));
                        self.job.carry = Some(p);
                        next = Some((Act::Fetch, Duration::from_secs(60)));
                    }
                    Err(e) => self.ctl.lock().unwrap().note(format!("carry refused: {e}")),
                }
            } else if tk.mischief
                && tk.now >= self.job.next_carry
                && matches!(self.bd.mode, Mode::Ground(Surface::Floor))
                && tk.user_active
                && tk.mood != Mood::Waiting
                && self.mind.needs.sleepiness < 0.7
            {
                match plan_carry(&self.world, &mut self.tape, &mut self.rng, self.bd.y, tk.half_body) {
                    Ok(p) => {
                        self.ctl.lock().unwrap().note(format!("mischief: '{}' {:+} px", p.name, p.d as f64 * p.dist));
                        self.job.carry = Some(p);
                        next = Some((Act::Fetch, Duration::from_secs(60)));
                        self.job.next_carry = tk.now + Duration::from_secs_f64(self.rng.gen_range(3600.0..7200.0) / self.time_scale);
                    }
                    Err(e) => {
                        self.ctl.lock().unwrap().note(format!("mischief skipped: {e}"));
                        self.job.next_carry = tk.now + Duration::from_secs_f64(600.0 / self.time_scale);
                    }
                }
            }
            let in_bed = self.bd.mode == Mode::Ground(Surface::Bed);
            if next.is_some() {
            } else if in_bed {
                // in bed: doze on, groom, or hop out (Hermes waiting / the user back: out to see them)
                let choice = if self.mind.needs.sleepiness > 0.85 {
                    "sleep"
                } else if tk.mood == Mood::Waiting || (self.mind.needs.boredom > 0.8 && tk.user_active) {
                    "out"
                } else {
                    pick(&mut self.rng, &[(0.55, "out"), (0.2, "groom"), (0.25, "idle")])
                };
                match choice {
                    "sleep" => next = Some((Act::Sleep, secs(&mut self.rng, 240.0, 600.0))),
                    "out" => {
                        self.bd.vx = if self.rng.gen_bool(0.5) { 280.0 } else { -280.0 };
                        self.bd.vy = -560.0;
                        self.bd.mode = Mode::Air;
                        next = Some((Act::Idle, secs(&mut self.rng, 2.0, 5.0)));
                    }
                    "groom" if self.sheet.has("groom") => next = Some((Act::Beat("groom"), beat_dur(&self.sheet, "groom", 4000))),
                    _ => next = Some((Act::Idle, secs(&mut self.rng, 3.0, 8.0))),
                }
            } else if self.mind.needs.sleepiness > 0.85 {
                // usually off to her bed, but she can doze off anywhere; a close cat often naps by the user's cursor
                if self.bond.close() && tk.user_active && self.rng.gen_bool(0.35) {
                    self.mind.nap_by_cursor = true;
                    next = Some((Act::Come, Duration::from_secs(40)));
                } else if tk.bed_ready && self.rng.gen_bool(0.75) {
                    self.job.bed_deadline = tk.now + Duration::from_secs(60);
                    next = Some((Act::ToBed, Duration::from_secs(60)));
                } else {
                    next = Some((Act::Sleep, secs(&mut self.rng, 240.0, 600.0)));
                }
            } else if tk.mood == Mood::Waiting && !self.mind.attended_waiting {
                self.mind.attended_waiting = true;
                next = Some((Act::Come, Duration::from_secs(40)));
            } else if tk.mood == Mood::Waiting {
                next = Some((Act::Attend, secs(&mut self.rng, 6.0, 14.0).mul_f64(0.6 + 0.8 * self.bond.v)));
            } else if self.mind.needs.boredom > 0.8 && tk.user_active {
                self.mind.needs.boredom = 0.0;
                if self.bond.wary() && self.rng.gen_bool(0.6) {
                    // not sure about him: watches from where she is
                    self.bd.face = if tk.cx < self.bd.x { -1 } else { 1 };
                    next = Some((Act::Look, secs(&mut self.rng, 3.0, 6.0)));
                } else {
                    next = Some((Act::Come, Duration::from_secs(40)));
                }
            } else if self.mind.needs.playfulness > 0.75 && self.mind.needs.sleepiness < 0.6 && self.rng.gen_bool(0.5) {
                self.mind.needs.playfulness = (self.mind.needs.playfulness - 0.6).max(0.0);
                let d = if self.rng.gen_bool(0.5) { 1 } else { -1 };
                next = Some((Act::Zoomies(d), secs(&mut self.rng, 4.0, 8.0)));
            } else {
                let sleepy = self.mind.needs.sleepiness > 0.6;
                let mut table: Vec<(f64, &str)> = match tk.mood {
                    Mood::Run => vec![(0.55, "walk"), (0.2, "look"), (0.2, "idle"), (0.05, "wave")],
                    Mood::Review => vec![(0.2, "walk"), (0.45, "look"), (0.35, "idle")],
                    _ if sleepy => vec![(0.2, "walk"), (0.15, "look"), (0.65, "idle")],
                    _ => vec![(0.45, "walk"), (0.13, "look"), (0.35, "idle"), (0.07, "wave")],
                };
                if self.sheet.has("groom") && tk.mood != Mood::Run {
                    table.push((0.08, "groom"));
                }
                if !sleepy {
                    if on_window {
                        if self.can_leap {
                            table.push((0.10, "down"));
                        }
                    } else {
                        if self.can_leap {
                            table.push((0.12, "jump"));
                        }
                        table.push((0.03, "climb"));
                        if tk.bed_ready && matches!(self.bed.mode, BedMode::Ground(Surface::Floor)) && matches!(self.bd.mode, Mode::Ground(Surface::Floor)) && (self.bed.x - self.bd.x).abs() < 1600.0 {
                            table.push((0.03, "movebed"));
                        }
                    }
                }
                match pick(&mut self.rng, &table) {
                    "walk" => {
                        let c = dir_toward_center();
                        let d = if self.rng.gen_bool(0.65) { c } else { -c };
                        next = Some((Act::Walk(d), secs(&mut self.rng, 3.0, 10.0)));
                    }
                    "look" => next = Some((Act::Look, secs(&mut self.rng, 2.0, 4.0))),
                    "wave" => next = Some((Act::Beat("wave"), beat_dur(&self.sheet, "wave", 2200))),
                    "groom" => {
                        let d = secs(&mut self.rng, 3.0, 6.0);
                        next = Some((Act::Beat("groom"), self.sheet.once_ms("groom").map_or(d, |m| Duration::from_millis(m as u64))));
                    }
                    "jump" => {
                        if let Some((tx, ty)) = self.world.jump_target(&mut self.rng, self.bd.x, self.bd.y) {
                            if self.has_takeoff {
                                self.bd.takeoff = Some((tx, ty));
                                self.bd.face = if tx < self.bd.x { -1 } else { 1 }; // crouch facing where she will fly
                                next = Some((Act::Beat("jump"), beat_dur(&self.sheet, "jump", 300)));
                            } else {
                                (self.bd.vx, self.bd.vy) = ballistic(self.bd.x, self.bd.y, tx, ty);
                                self.bd.mode = Mode::Air;
                                next = Some((Act::Idle, secs(&mut self.rng, 2.0, 5.0)));
                            }
                        }
                    }
                    "down" => {
                        // walk to the nearer edge and hop off there (a hop from the middle of the window
                        // landed her right back on it: a jump into a wall)
                        let d = self.bd.on_rect.map_or(1, |r| if self.bd.x - (r.left as f64) < r.right as f64 - self.bd.x { -1 } else { 1 });
                        self.job.get_down = Some(d);
                        next = Some((Act::Walk(d), Duration::from_secs(30)));
                    }
                    "climb" => {
                        if let Some(d) = outer_wall(&self.world.mons, &self.bd.mon, self.bd.x) {
                            next = Some((Act::ToWall(d), Duration::from_secs(60)));
                        }
                    }
                    "movebed" => {
                        if let Some(d) = drag_dir(&self.world, &self.bed, tk.k, tk.half_body) {
                            next = Some((Act::FetchBed(d), Duration::from_secs(40)));
                        }
                    }
                    _ => {}
                }
                if next.is_none() {
                    next = Some((Act::Idle, secs(&mut self.rng, 2.0, 6.0)));
                }
            }
            if let Some((a, d)) = next {
                self.mind.act = a;
                self.mind.act_until = tk.now + d;
            }
        }

    }
}
