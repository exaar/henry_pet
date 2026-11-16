//! Phase 3: her body moves - held, flying, riding a window, in bed, on a perch, walking the floor, climbing.
use super::*;
use crate::icons;
use crate::win::{self, Monitor};
use rand::Rng;
use std::time::Duration;

impl Brain {
    /// Her body moves: held, flying, riding a window, in bed, on a perch, walking the floor, climbing.
    pub(super) fn motion(&mut self, tk: &mut Tick) {
        // --- body ---
        // where she stood before this tick's motion: a transition picked further down this tick starts
        // from here (the walk that asked for a get-up must not have moved her yet)
        tk.x_tick0 = self.bd.x;
        tk.speed = 55.0 * tk.size as f64 / 64.0;
        // sitting down / getting up / lying down: stay put. Also on the tick a transition ends: the next
        // one (trans_stand -> trans_walk) is only picked further down, and a tick of walk speed here
        // shoved her ~3 px forward right before the first step (2026-11-16)
        if self.ps.busy() {
            tk.speed = 0.0;
        }
        if tk.mood == Mood::Run {
            tk.speed *= 1.3;
        }
        if self.mind.needs.sleepiness > 0.6 {
            tk.speed *= 0.75;
        }
        tk.gap = self.sprite.cell_h * tk.k - self.sprite.foot_y * tk.k; // transparent rows under her feet
        if !matches!(self.bd.mode, Mode::Ground(Surface::Window(_))) {
            self.job.get_down = None; // off the window (hopped, fell, picked up): done
        }
        match self.bd.mode {
            Mode::Held(ox, oy) => {
                let held_pivot = self.sheet.anims.iter().find(|a| crate::render::in_group(&a.name, "held")).and_then(|a| a.pivot);
                if let (Some(sw), Some((pvx, pvy))) = (self.bd.swing.as_mut(), held_pivot) {
                    sw.step((tk.cx, tk.cy), tk.dt);
                    // feet of the upright held frame (what she falls with when let go)
                    let (sx, sy) = sw.scruff();
                    self.bd.x = sx - pvx * tk.k + tk.w as f64 / 2.0;
                    self.bd.y = sy - pvy * tk.k + self.sprite.foot_y * tk.k;
                } else {
                    self.bd.x = tk.cx + ox;
                    self.bd.y = tk.cy + oy;
                }
                self.bd.samples.push_back((tk.now, tk.cx, tk.cy));
                while self.bd.samples.len() > 2 && tk.now - self.bd.samples[0].0 > Duration::from_millis(90) {
                    self.bd.samples.pop_front();
                }
                if !tk.lbutton {
                    let (t0, x0, y0) = self.bd.samples[0];
                    let span = (tk.now - t0).as_secs_f64().max(0.016);
                    self.bd.vx = ((tk.cx - x0) / span).clamp(-4000.0, 4000.0);
                    self.bd.vy = ((tk.cy - y0) / span).clamp(-4000.0, 4000.0);
                    self.bd.mode = Mode::Air;
                    self.bd.swing = None;
                    // a quick flick at the end of a carry is not a throw (PLAN #64)
                    self.touch.tossed = self.bd.vx.abs() + self.bd.vy.abs() > crate::bond::THROW_SPEED;
                    if self.touch.tossed {
                        self.bond.add(crate::bond::THROWN);
                    }
                    let ms = self.tape.val_or("movesize", || false, || win::in_move_size(self.hwnd));
                    self.ctl.lock().unwrap().note(format!("thrown v=({:.0},{:.0}) after {} ms, {} samples, movesize {ms}", self.bd.vx, self.bd.vy, (tk.now - self.bd.held_since).as_millis(), self.bd.samples.len()));
                }
            }
            Mode::Air => {
                self.bd.vy += G * tk.dt;
                let mut nx = self.bd.x + self.bd.vx * tk.dt;
                let mut ny = self.bd.y + self.bd.vy * tk.dt;
                // walls: bounce off the outer edges of the desktop
                if monitor_at(&self.world.mons, nx, ny.min(self.bd.y)).is_none() {
                    self.bd.vx = -self.bd.vx * 0.45;
                    nx = self.bd.x;
                }
                if let Some(m) = monitor_at(&self.world.mons, nx, ny) {
                    let ceil = m.rect.top as f64 + tk.h as f64 * 0.6;
                    if ny < ceil {
                        ny = ceil;
                        self.bd.vy = self.bd.vy.max(0.0);
                    }
                    let (l, r) = (m.rect.left as f64 + tk.half_body, m.rect.right as f64 - tk.half_body);
                    let neighbour = |px: f64| self.world.mons.iter().any(|o| o.handle != m.handle && inside(&o.rect, px, ny.min(o.rect.bottom as f64 - 1.0)));
                    if nx < l && !neighbour(m.rect.left as f64 - 2.0) {
                        nx = l;
                        self.bd.vx = self.bd.vx.abs() * 0.45;
                    } else if nx > r && !neighbour(m.rect.right as f64 + 1.0) {
                        nx = r;
                        self.bd.vx = -self.bd.vx.abs() * 0.45;
                    }
                }
                // the jump into her bed: land in it if it is still there
                let mut in_bed_now = false;
                if self.job.bed_jump && self.bd.vy > 0.0 && tk.bed_ready {
                    let (tx, ty) = self.bed.foot(tk.k, self.bd.face);
                    if self.bd.y <= ty + 2.0 && ny >= ty && (nx - tx).abs() < 60.0 * tk.k {
                        let impact = self.bd.vy;
                        nx = tx;
                        ny = ty;
                        self.bd.mode = Mode::Ground(Surface::Bed);
                        (self.bd.vx, self.bd.vy) = (0.0, 0.0);
                        self.job.bed_jump = false;
                        self.bed.jelly.kick(0.0, 0.6, 30.0 + impact * 0.02, 1.2 + impact * 0.0012);
                        self.mind.act = Act::Sleep;
                        self.mind.act_until = tk.now + secs(&mut self.rng, 240.0, 600.0);
                        in_bed_now = true;
                        self.ctl.lock().unwrap().note("hopped into bed".into());
                    }
                }
                let perched = in_bed_now || match self.bd.perch_target {
                    Some((ptx, pty)) if self.bd.vy > 0.0 && self.bd.y <= pty && ny >= pty && (nx - ptx).abs() < 140.0 => {
                        ny = pty;
                        nx = ptx;
                        self.bd.mode = Mode::Ground(Surface::Perch(pty.round() as i32));
                        (self.bd.vx, self.bd.vy) = (0.0, 0.0);
                        self.bd.perch_target = None;
                        if let Some(c) = self.job.carry.as_mut() {
                            c.start_x = nx;
                        }
                        self.mind.act = Act::Carry;
                        self.mind.act_until = tk.now + Duration::from_secs(20);
                        true
                    }
                    _ => false,
                };
                if self.bd.vy > 0.0 && !perched {
                    if let Some((s, sy)) = self.world.landing(nx, self.bd.y, ny) {
                        self.job.bed_jump = false;
                        if self.bd.perch_target.take().is_some() {
                            self.job.carry = None;
                            self.ctl.lock().unwrap().note("carry aborted: landed elsewhere".into());
                        }
                        let land_flop = self.bd.vy > 2300.0;
                        ny = sy;
                        self.bd.mode = Mode::Ground(s);
                        self.bd.on_rect = match s {
                            Surface::Window(hw) => self.world.win(hw).map(|(_, wi)| wi.rect),
                            Surface::Floor | Surface::Perch(_) | Surface::Bed => None,
                        };
                        (self.bd.vx, self.bd.vy) = (0.0, 0.0);
                        // only a splat the user caused counts; her own falls (window gone, hops) don't
                        if land_flop && self.touch.tossed {
                            self.bond.add(crate::bond::SPLAT);
                        }
                        self.touch.tossed = false;
                        if land_flop {
                            self.mind.act = Act::Beat("splat");
                            self.mind.act_until = tk.now + beat_dur(&self.sheet, "splat", 1600);
                        } else if !matches!(self.mind.act, Act::Sleep | Act::Come | Act::ToCorner(_) | Act::ToBed | Act::FetchBed(_) | Act::ToWarm) {
                            if self.sheet.has("land") {
                                self.mind.act = Act::Beat("land");
                                self.mind.act_until = tk.now + beat_dur(&self.sheet, "land", 300);
                            } else {
                                self.mind.act = Act::Idle;
                                self.mind.act_until = tk.now + secs(&mut self.rng, 1.0, 3.0);
                            }
                        }
                    }
                }
                // fell out of every monitor (e.g. one was unplugged): put her back on the primary taskbar
                if monitor_at(&self.world.mons, nx, ny - 2.0).is_none() && monitor_at(&self.world.mons, nx, self.bd.y).is_none() {
                    if let Some(p) = self.world.mons.iter().find(|m| m.primary).or(self.world.mons.first()) {
                        nx = (p.work.left + p.work.right) as f64 / 2.0;
                        ny = p.work.bottom as f64;
                        self.bd.mode = Mode::Ground(Surface::Floor);
                        (self.bd.vx, self.bd.vy) = (0.0, 0.0);
                    }
                }
                self.bd.x = nx;
                self.bd.y = ny;
            }
            Mode::Ground(Surface::Window(hw)) => {
                // ride the window; fall when it closes, minimizes, maximizes or gets covered
                let fall = match self.world.win(hw) {
                    Some((i, wi)) => {
                        if let Some(old) = self.bd.on_rect {
                            self.bd.x += (wi.rect.left - old.left) as f64;
                        }
                        self.bd.y = wi.rect.top as f64;
                        self.bd.on_rect = Some(wi.rect);
                        !self.world.standable(i, self.bd.x)
                    }
                    None => true,
                };
                if fall {
                    self.bd.mode = Mode::Air;
                    (self.bd.vx, self.bd.vy) = (0.0, 0.0);
                    self.ctl.lock().unwrap().note("window gone -> falling".into());
                    if self.mind.act == Act::Sleep {
                        self.mind.act = Act::Idle;
                    }
                } else {
                    let (l, r) = self.bd.on_rect.map(|r| (r.left as f64, r.right as f64)).unwrap();
                    let walk_off = |d: i8, x: f64| -> bool {
                        // at the edge: turn, or walk off it
                        let edge = if d > 0 { r - tk.half_body * 0.6 } else { l + tk.half_body * 0.6 };
                        if d > 0 { x >= edge } else { x <= edge }
                    };
                    let mv = |d: i8, s: f64, x: &mut f64| *x += d as f64 * s * tk.dt;
                    // hopping down off the edge: fast enough to clear the window (she can't stand within 8 px
                    // of its sides). The old hop (walk speed, 0.1 s in the air) covered ~19 px of the ~45 left,
                    // so she landed back on the same window - 35 of 40 walk-offs in a live recording looked
                    // like a jump into a wall
                    let hop_off = |d: i8, x: f64, s: f64| -> (f64, f64) {
                        let vy = -300.0;
                        let air = 2.0 * -vy / G;
                        let clear = if d > 0 { r - 8.0 - x } else { x - (l + 8.0) };
                        (d as f64 * s.max((clear.max(0.0) + tk.half_body * 0.3) / air), vy)
                    };
                    match self.mind.act {
                        Act::Walk(d) => {
                            mv(d, tk.speed, &mut self.bd.x);
                            if walk_off(d, self.bd.x) {
                                if self.job.get_down == Some(d) || self.rng.gen_bool(0.35) {
                                    (self.bd.vx, self.bd.vy) = hop_off(d, self.bd.x, tk.speed);
                                    self.bd.mode = Mode::Air;
                                } else {
                                    self.mind.act = Act::Walk(-d);
                                }
                            }
                        }
                        Act::Zoomies(d) => {
                            mv(d, tk.speed * 2.4, &mut self.bd.x);
                            if walk_off(d, self.bd.x) {
                                self.mind.act = Act::Zoomies(-d);
                            }
                        }
                        Act::ToBed | Act::FetchBed(_) | Act::ToWarm => {
                            // walk toward the bed / warm window (off this window if it is not up here)
                            let goal = if self.mind.act == Act::ToWarm { self.job.warm_tx } else { self.bed.x };
                            let d: i8 = self.job.get_down.unwrap_or(if goal < self.bd.x { -1 } else { 1 });
                            mv(d, tk.speed, &mut self.bd.x);
                            if walk_off(d, self.bd.x) {
                                (self.bd.vx, self.bd.vy) = hop_off(d, self.bd.x, tk.speed);
                                self.bd.mode = Mode::Air;
                            }
                        }
                        Act::ToCorner(d) | Act::ToWall(d) => {
                            mv(d, tk.speed, &mut self.bd.x);
                            if walk_off(d, self.bd.x) {
                                (self.bd.vx, self.bd.vy) = hop_off(d, self.bd.x, tk.speed);
                                self.bd.mode = Mode::Air;
                            }
                        }
                        Act::Come => {
                            let gapx = tk.cx - self.bd.x;
                            if gapx.abs() < 24.0 + tk.half_body * 0.5 || tk.now >= self.mind.act_until {
                                if self.mind.nap_by_cursor {
                                    self.mind.nap_by_cursor = false;
                                    self.mind.act = Act::Sleep;
                                    self.mind.act_until = tk.now + secs(&mut self.rng, 240.0, 600.0);
                                } else {
                                    self.mind.act = Act::Beat("wave");
                                    self.mind.act_until = tk.now + beat_dur(&self.sheet, "wave", 2200);
                                    tk.want_meow = self.mind.greet;
                                    self.mind.greet = false;
                                }
                                self.mind.needs.boredom = 0.0;
                            } else {
                                let d: i8 = if gapx > 0.0 { 1 } else { -1 };
                                mv(d, tk.speed * 1.4, &mut self.bd.x);
                                if walk_off(d, self.bd.x) {
                                    (self.bd.vx, self.bd.vy) = hop_off(d, self.bd.x, tk.speed * 1.4);
                                    self.bd.mode = Mode::Air;
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Mode::Ground(Surface::Bed) => {
                // lying in it: the bed carries her (it rides windows, sags under her)
                let (bx, by) = self.bed.foot(tk.k, self.bd.face);
                self.bd.x = bx;
                self.bd.y = by;
                let going = match self.mind.act {
                    Act::Walk(d) | Act::Zoomies(d) | Act::ToCorner(d) | Act::ToWall(d) | Act::FetchBed(d) | Act::DragBed(d) => Some(d),
                    Act::Come | Act::Fetch | Act::Carry => Some(if tk.cx < self.bd.x { -1 } else { 1 }),
                    Act::ToWarm => Some(if self.job.warm_tx < self.bd.x { -1 } else { 1 }),
                    _ => None,
                };
                if let Some(d) = going {
                    // asked to go somewhere: hop out first
                    self.bd.vx = d as f64 * 280.0;
                    self.bd.vy = -560.0;
                    self.bd.mode = Mode::Air;
                }
            }
            Mode::Ground(Surface::Perch(py)) => {
                self.bd.y = py as f64;
                let mut finished: Option<String> = None;
                if self.mind.act == Act::Carry && self.job.carry.is_some() {
                    let c = self.job.carry.as_mut().unwrap();
                    self.bd.x += c.d as f64 * tk.speed * 0.45 * tk.dt;
                    let mut dx = (self.bd.x - c.start_x).round() as i32;
                    let done = dx.abs() as f64 >= c.dist || tk.now >= self.mind.act_until;
                    if done {
                        dx = dx.signum() * (dx.abs().min(c.dist as i32));
                    }
                    if dx != c.last_dx {
                        if !c.recorded {
                            if !self.tape.replaying() {
                                icons::record_original(&tk.icons_path, &c.name, c.pos, &c.all);
                            }
                            c.recorded = true;
                        }
                        let (lv, index, px, py) = (c.lv, c.index, c.pos.0 + dx, c.pos.1);
                        if !self.tape.val_or("setpos", || true, || icons::set_pos(lv, index, px, py)) {
                            finished = Some(format!("carry stopped: explorer refused the move of '{}'", c.name));
                        }
                        c.last_dx = dx;
                    }
                    if done && finished.is_none() {
                        finished = Some(format!("carried '{}' {:+} px", c.name, dx));
                    }
                } else if self.mind.act == Act::Look && tk.now < self.mind.act_until {
                    // admiring her work
                } else {
                    // get down
                    self.bd.vx = if self.rng.gen_bool(0.5) { 230.0 } else { -230.0 };
                    self.bd.vy = -420.0;
                    self.bd.mode = Mode::Air;
                    self.job.carry = None;
                }
                if let Some(msg) = finished {
                    self.ctl.lock().unwrap().note(msg);
                    self.job.carry = None;
                    self.mind.act = Act::Look;
                    self.mind.act_until = tk.now + Duration::from_millis(1500);
                }
            }
            Mode::Ground(Surface::Floor) => {
                self.bd.y = self.bd.mon.work.bottom as f64;
                if self.mind.act == Act::Fetch {
                    match self.job.carry.as_ref() {
                        Some(c) if tk.now < self.mind.act_until => {
                            let gapx = c.lx - self.bd.x;
                            if gapx.abs() < 500.0 {
                                (self.bd.vx, self.bd.vy) = ballistic(self.bd.x, self.bd.y, c.lx, c.perch_y);
                                self.bd.perch_target = Some((c.lx, c.perch_y));
                                self.bd.mode = Mode::Air;
                                self.mind.act = Act::Idle;
                                self.mind.act_until = tk.now + Duration::from_secs(10);
                            } else {
                                self.bd.x += gapx.signum() * tk.speed * 1.2 * tk.dt;
                                if let Some(m) = monitor_at(&self.world.mons, self.bd.x, self.bd.y - 2.0) {
                                    self.bd.mon = m.clone();
                                }
                            }
                        }
                        _ => {
                            self.job.carry = None;
                            self.mind.act = Act::Idle;
                        }
                    }
                }
                let step = |x: &mut f64, mon: &mut Monitor, d: i8, dist: f64, cross: bool| -> bool {
                    // returns true at an edge she can't pass
                    let nx = *x + d as f64 * dist;
                    let (edge, probe) = if d > 0 {
                        (mon.work.right as f64, mon.work.right as f64 + 1.0)
                    } else {
                        (mon.work.left as f64, mon.work.left as f64 - 2.0)
                    };
                    let neighbour = self.world
                        .mons
                        .iter()
                        .find(|m| {
                            m.handle != mon.handle
                                && (m.work.left as f64) <= probe
                                && probe < m.work.right as f64
                                && Some(m.handle) != self.sense.fullscreen
                        })
                        .cloned();
                    let crossing = cross && neighbour.is_some();
                    let limit = if crossing { edge } else { edge - d as f64 * tk.half_body };
                    let past = if d > 0 { nx >= limit } else { nx <= limit };
                    if !past {
                        *x = nx;
                        false
                    } else if crossing {
                        *x = nx;
                        *mon = neighbour.unwrap();
                        false
                    } else {
                        *x = limit;
                        true
                    }
                };
                match self.mind.act {
                    Act::Walk(d) => {
                        if step(&mut self.bd.x, &mut self.bd.mon, d, tk.speed * tk.dt, true) {
                            self.mind.act = Act::Walk(-d);
                        }
                    }
                    Act::Zoomies(d) => {
                        if step(&mut self.bd.x, &mut self.bd.mon, d, tk.speed * 2.4 * tk.dt, false) {
                            self.mind.act = Act::Zoomies(-d);
                        }
                    }
                    Act::ToCorner(d) => {
                        if step(&mut self.bd.x, &mut self.bd.mon, d, tk.speed * tk.dt, false) {
                            self.mind.act = Act::Rest;
                        }
                    }
                    Act::ToBed => {
                        // the bed is far or up on a window out of reach from here: walk under it
                        let gapx = self.bed.x - self.bd.x;
                        let d: i8 = if gapx < 0.0 { -1 } else { 1 };
                        if step(&mut self.bd.x, &mut self.bd.mon, d, (tk.speed * 1.1 * tk.dt).min(gapx.abs()), true) {
                            self.mind.act = Act::Sleep; // can't get there: nap here
                            self.mind.act_until = tk.now + secs(&mut self.rng, 240.0, 600.0);
                        }
                    }
                    Act::ToWarm => {
                        let gapx = self.job.warm_tx - self.bd.x;
                        if gapx.abs() >= 2.0 {
                            let d: i8 = if gapx < 0.0 { -1 } else { 1 };
                            if step(&mut self.bd.x, &mut self.bd.mon, d, (tk.speed * tk.dt).min(gapx.abs()), true) {
                                self.mind.act = Act::Sleep; // can't get under it: nap here
                                self.mind.act_until = tk.now + secs(&mut self.rng, 240.0, 600.0);
                                self.job.warm = None;
                                self.job.next_warm = tk.now + Duration::from_secs(300);
                            }
                        }
                    }
                    Act::FetchBed(_) => {
                        let gapx = self.job.fetch_tx - self.bd.x;
                        if gapx.abs() >= 4.0 {
                            let d: i8 = if gapx < 0.0 { -1 } else { 1 };
                            if step(&mut self.bd.x, &mut self.bd.mon, d, (tk.speed * tk.dt).min(gapx.abs()), true) {
                                self.mind.act = Act::Look; // the end she wanted to bite is out of reach
                                self.mind.act_until = tk.now + Duration::from_secs(2);
                            }
                        }
                    }
                    Act::DragBed(d) => {
                        // the bed goes in front of her: stop before it would leave the monitor
                        let half = crate::bed::BW / 2.0 * tk.k;
                        let ahead = self.bed.x + d as f64 * half;
                        let room = if d > 0 { self.bd.mon.work.right as f64 - 12.0 - ahead } else { ahead - self.bd.mon.work.left as f64 - 12.0 };
                        let stuck = room <= 0.0 || step(&mut self.bd.x, &mut self.bd.mon, d, tk.speed * 0.7 * tk.dt, false);
                        if stuck || tk.now >= self.mind.act_until {
                            let mut jumped = false;
                            if self.can_leap && self.has_takeoff && !stuck && self.rng.gen_bool(0.4) {
                                // up onto a window with it in her teeth
                                if let Some((tx, ty)) = self.world.jump_target(&mut self.rng, self.bd.x, self.bd.y) {
                                    if (tx - self.bd.x).signum() as i8 == d {
                                        self.bd.takeoff = Some((tx, ty));
                                        self.mind.act = Act::Beat("jump");
                                        self.mind.act_until = tk.now + beat_dur(&self.sheet, "jump", 300);
                                        jumped = true;
                                    }
                                }
                            }
                            if !jumped {
                                self.bed.drop_it(0.0, 0.0);
                                self.ctl.lock().unwrap().note("bed dropped".into());
                                self.mind.act = Act::Look;
                                self.mind.act_until = tk.now + Duration::from_secs(2);
                            }
                        }
                    }
                    Act::ToWall(d) => {
                        if step(&mut self.bd.x, &mut self.bd.mon, d, tk.speed * tk.dt, false) {
                            let wall = if d > 0 { self.bd.mon.work.right } else { self.bd.mon.work.left } as f64;
                            self.bd.x = wall;
                            self.bd.y = self.bd.mon.work.bottom as f64 - tk.w as f64 / 2.0;
                            let span = (self.bd.mon.work.bottom - self.bd.mon.work.top) as f64;
                            let target = self.bd.mon.work.top as f64 + span * self.rng.gen_range(0.25..0.6);
                            self.bd.mode = Mode::Climb(d, target);
                            self.mind.act = Act::Idle;
                        }
                    }
                    Act::Come => {
                        let gapx = tk.cx - self.bd.x;
                        if gapx.abs() < 24.0 + tk.half_body * 0.5 || tk.now >= self.mind.act_until {
                            if self.mind.nap_by_cursor {
                                self.mind.nap_by_cursor = false;
                                self.mind.act = Act::Sleep;
                                self.mind.act_until = tk.now + secs(&mut self.rng, 240.0, 600.0);
                            } else {
                                self.mind.act = Act::Beat("wave");
                                self.mind.act_until = tk.now + beat_dur(&self.sheet, "wave", 2200);
                                tk.want_meow = self.mind.greet;
                                self.mind.greet = false;
                            }
                            self.mind.needs.boredom = 0.0;
                        } else {
                            let d: i8 = if gapx > 0.0 { 1 } else { -1 };
                            if step(&mut self.bd.x, &mut self.bd.mon, d, tk.speed * 1.4 * tk.dt, true) {
                                self.mind.act = Act::Attend;
                                self.mind.act_until = tk.now + secs(&mut self.rng, 6.0, 12.0);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Mode::Climb(d, target) => {
                self.bd.y -= tk.speed * 0.7 * tk.dt;
                if self.bd.y <= target {
                    self.bd.mode = Mode::Cling(d);
                    self.bd.cling_until = tk.now + secs(&mut self.rng, 3.0, 9.0);
                }
            }
            Mode::Cling(d) => {
                if tk.now >= self.bd.cling_until {
                    // let go: turn feet-down and drop, pushing off the wall a little
                    self.bd.y += tk.w as f64 / 2.0 + tk.gap;
                    self.bd.x -= d as f64 * tk.w as f64 * 0.6;
                    self.bd.vx = -(d as f64) * 220.0;
                    self.bd.vy = -200.0;
                    self.bd.mode = Mode::Air;
                }
            }
        }
        if self.mind.act != Act::Come {
            self.mind.nap_by_cursor = false; // the nap-by-the-cursor trip ended some other way
        }
        // a wave that ended a Come turns into attending
        if let Act::Beat("wave") = self.mind.act {
            if tk.grounded && tk.now >= self.mind.act_until && (tk.mood == Mood::Waiting || self.mind.needs.boredom == 0.0) && tk.user_active {
                self.mind.act = Act::Attend;
                // a close cat stays with him longer (x0.6 .. x1.4)
                self.mind.act_until = tk.now + secs(&mut self.rng, 6.0, 12.0).mul_f64(0.6 + 0.8 * self.bond.v);
                self.mind.needs.boredom = 0.01;
            }
        }

    }
}
