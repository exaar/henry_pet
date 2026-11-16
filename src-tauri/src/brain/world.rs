//! The desktop she lives on: monitors, window tops she can stand on, jumps, icons to push, warm windows.
use super::*;
use crate::icons;
use crate::win::{self, Monitor, Rect, WinInfo};
use rand::Rng;
use std::collections::BTreeMap;
use std::time::Duration;
use crate::tape::Tape;

pub(crate) fn secs(rng: &mut impl Rng, lo: f64, hi: f64) -> Duration {
    Duration::from_secs_f64(rng.gen_range(lo..hi))
}

pub(crate) fn pick<T: Copy>(rng: &mut impl Rng, table: &[(f64, T)]) -> T {
    let total: f64 = table.iter().map(|(w, _)| w).sum();
    let mut r = rng.gen_range(0.0..total);
    for (w, v) in table {
        if r < *w {
            return *v;
        }
        r -= w;
    }
    table[table.len() - 1].1
}

pub(crate) fn inside(r: &Rect, x: f64, y: f64) -> bool {
    (r.left as f64) <= x && x < r.right as f64 && (r.top as f64) <= y && y < r.bottom as f64
}

pub(crate) fn monitor_at(mons: &[Monitor], x: f64, y: f64) -> Option<&Monitor> {
    mons.iter().find(|m| inside(&m.rect, x, y)).or_else(|| {
        mons.iter().find(|m| (m.rect.left as f64) <= x && x < m.rect.right as f64)
    })
}

/// The world she moves in, refreshed ~10x per second.
pub(crate) struct World {
    pub(crate) mons: Vec<Monitor>,
    /// Top-level windows, topmost first.
    pub(crate) wins: Vec<WinInfo>,
    /// Sprite size on screen.
    pub(crate) h: f64,
}

impl World {
    pub(crate) fn win(&self, hwnd: isize) -> Option<(usize, &WinInfo)> {
        self.wins.iter().enumerate().find(|(_, w)| w.hwnd == hwnd)
    }

    /// Can she stand on window `i` at x: a real top edge with room above, not covered by a window above it.
    pub(crate) fn standable(&self, i: usize, x: f64) -> bool {
        let w = &self.wins[i];
        if w.maximized || x < w.rect.left as f64 + 8.0 || x > w.rect.right as f64 - 8.0 {
            return false;
        }
        let top = w.rect.top as f64;
        let Some(m) = monitor_at(&self.mons, x, top + 1.0) else { return false };
        if top < m.work.top as f64 + self.h * 0.8 || top > m.work.bottom as f64 - 40.0 {
            return false;
        }
        !self.wins[..i].iter().any(|o| inside(&o.rect, x, top + 3.0) || inside(&o.rect, x, top - self.h * 0.45))
    }

    /// First surface her feet cross going down from y0 to y1 at x.
    pub(crate) fn landing(&self, x: f64, y0: f64, y1: f64) -> Option<(Surface, f64)> {
        let mut best: Option<(Surface, f64)> = None;
        if let Some(m) = monitor_at(&self.mons, x, y0.max(0.0).min(y1)) {
            let b = m.work.bottom as f64;
            if y1 >= b {
                best = Some((Surface::Floor, b));
            }
        }
        for (i, w) in self.wins.iter().enumerate() {
            let top = w.rect.top as f64;
            if y0 <= top && top <= y1 && best.map_or(true, |(_, b)| top < b) && self.standable(i, x) {
                best = Some((Surface::Window(w.hwnd), top));
            }
        }
        best
    }

    /// A window top in jumping reach: (target x, target top).
    pub(crate) fn jump_target(&self, rng: &mut impl Rng, x: f64, y: f64) -> Option<(f64, f64)> {
        let mut cands = Vec::new();
        for (i, w) in self.wins.iter().enumerate() {
            let top = w.rect.top as f64;
            let rise = y - top;
            if !(60.0..1350.0).contains(&rise) {
                continue;
            }
            let (l, r) = (w.rect.left as f64 + 40.0, w.rect.right as f64 - 40.0);
            if r <= l {
                continue;
            }
            for _ in 0..4 {
                let tx = rng.gen_range(l..r);
                if (tx - x).abs() < 1400.0 && self.standable(i, tx) {
                    cands.push((tx, top));
                    break;
                }
            }
        }
        if cands.is_empty() {
            None
        } else {
            Some(cands[rng.gen_range(0..cands.len())])
        }
    }
}

pub(crate) struct CarryPlan {
    pub(crate) lv: isize,
    pub(crate) index: i32,
    pub(crate) name: String,
    pub(crate) pos: (i32, i32),
    pub(crate) all: Vec<icons::Icon>,
    pub(crate) d: i8,
    pub(crate) dist: f64,
    /// Where she lands: behind the icon on its row.
    pub(crate) lx: f64,
    pub(crate) perch_y: f64,
    pub(crate) start_x: f64,
    pub(crate) recorded: bool,
    pub(crate) last_dx: i32,
}

pub(crate) fn overlaps(a: &Rect, b: &Rect) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

/// Pick an icon she can visibly drag 150-350 px along its row: fully on one monitor, nothing covering the
/// stretch, landing spot free of other icons, reachable by a jump from the taskbar.
pub(crate) fn plan_carry(world: &World, tape: &mut Tape, rng: &mut impl Rng, y: f64, half_body: f64) -> Result<CarryPlan, String> {
    let (lv, all, root) = tape.val_or("icons", || Err("replay: no desktop icons".to_string()), || -> Result<(isize, Vec<icons::Icon>, isize), String> {
        let lv = icons::desktop_listview().ok_or("no desktop list view")?;
        if let Some(why) = icons::blocked(lv) {
            return Err(why.into());
        }
        let all = icons::list(lv).ok_or("could not read icons")?;
        Ok((lv, all, win::root(lv)))
    })?;
    let body = (half_body * 2.0) as i32;
    let mut cands = Vec::new();
    let mut why: BTreeMap<&str, u32> = BTreeMap::new();
    for ic in &all {
        let r = ic.rect;
        let iw = (r.right - r.left) as f64;
        let Some(m) = monitor_at(&world.mons, (r.left + r.right) as f64 / 2.0, (r.top + r.bottom) as f64 / 2.0) else { continue };
        if r.left < m.work.left || r.right > m.work.right || r.top < m.work.top + world.h as i32 || r.bottom > m.work.bottom {
            *why.entry("off-screen/too high").or_default() += 1;
            continue;
        }
        let rise = y - r.bottom as f64;
        if !(0.0..1350.0).contains(&rise) {
            *why.entry("out of jump reach").or_default() += 1;
            continue;
        }
        let first: i8 = if rng.gen_bool(0.5) { 1 } else { -1 };
        for d in [first, -first] {
            let dist = rng.gen_range(150.0..350.0f64).round();
            let shift = (d as f64 * dist) as i32;
            let moved = Rect { left: r.left + shift, right: r.right + shift, ..r };
            let path = Rect {
                left: r.left.min(moved.left) - body,
                right: r.right.max(moved.right) + body,
                top: r.top - world.h as i32,
                bottom: r.bottom,
            };
            if path.left < m.work.left || path.right > m.work.right {
                *why.entry("no room").or_default() += 1;
                continue;
            }
            if world.wins.iter().take_while(|w| w.hwnd != root).any(|w| overlaps(&w.rect, &path)) {
                *why.entry("covered by a window").or_default() += 1;
                continue;
            }
            if all.iter().any(|o| o.index != ic.index && overlaps(&o.rect, &moved)) {
                *why.entry("would land on another icon").or_default() += 1;
                continue;
            }
            let cx = (r.left + r.right) as f64 / 2.0;
            cands.push((ic.clone(), d, dist, cx - d as f64 * (half_body + iw / 2.0 - 10.0), r.bottom as f64));
            break;
        }
    }
    if cands.is_empty() {
        return Err(format!("no free icon among {} ({why:?})", all.len()));
    }
    let (ic, d, dist, lx, perch_y) = cands.swap_remove(rng.gen_range(0..cands.len()));
    Ok(CarryPlan {
        lv,
        index: ic.index,
        name: ic.name,
        pos: ic.pos,
        all,
        d,
        dist,
        lx,
        perch_y,
        start_x: lx,
        recorded: false,
        last_dx: 0,
    })
}

/// Launch velocity for a jump from (x, y) that comes down onto (tx, ty).
pub(crate) fn ballistic(x: f64, y: f64, tx: f64, ty: f64) -> (f64, f64) {
    let apex = ty.min(y) - 90.0;
    let vy = -(2.0 * G * (y - apex)).sqrt();
    let t = -vy / G + (2.0 * (ty - apex) / G).sqrt();
    ((tx - x) / t, vy)
}

/// The windowed process loading the GPU most: (its biggest window, pid, mean load over `n` samples).
pub(crate) fn hottest(world: &World, gpu: &crate::gpu::Gpu, n: usize) -> Option<(WinInfo, f64)> {
    let own = std::process::id();
    let mut best: Option<(WinInfo, f64)> = None;
    for wi in &world.wins {
        if wi.pid == 0 || wi.pid == own {
            continue;
        }
        let load = gpu.mean(wi.pid, n);
        let area = |w: &WinInfo| (w.rect.right - w.rect.left) as i64 * (w.rect.bottom - w.rect.top) as i64;
        let better = match &best {
            None => true,
            Some((b, bl)) => load > *bl + 0.01 || (b.pid == wi.pid && area(wi) > area(b)),
        };
        if better && load > 0.0 {
            best = Some((wi.clone(), load));
        }
    }
    best
}

/// Where on the floor / window top she lies down for the warm window: its middle, kept on its monitor.
pub(crate) fn warm_x(world: &World, wi: &WinInfo, half_body: f64) -> f64 {
    let mid = (wi.rect.left + wi.rect.right) as f64 / 2.0;
    let mid_y = (wi.rect.top + wi.rect.bottom) as f64 / 2.0;
    match monitor_at(&world.mons, mid, mid_y) {
        Some(m) => mid.clamp(m.work.left as f64 + half_body, m.work.right as f64 - half_body),
        None => mid,
    }
}

/// An outer edge of this monitor (one with no monitor beyond it), nearest to x first.
pub(crate) fn outer_wall(mons: &[Monitor], mon: &Monitor, x: f64) -> Option<i8> {
    let mid = (mon.work.top + mon.work.bottom) as f64 / 2.0;
    let free = |d: i8| {
        let px = if d > 0 { mon.rect.right as f64 + 1.0 } else { mon.rect.left as f64 - 2.0 };
        !mons.iter().any(|m| inside(&m.rect, px, mid))
    };
    let near: i8 = if x - (mon.work.left as f64) < (mon.work.right as f64) - x { -1 } else { 1 };
    [near, -near].into_iter().find(|d| free(*d))
}

pub(crate) const G: f64 = 2600.0;

/// Where Henry's mouth is in her art (cell px, art facing right), per animation group: the bed hangs there.
pub(crate) fn mouth(anim: &str) -> (f64, f64) {
    if anim.starts_with("fly") {
        (110.0, 44.0)
    } else if anim.starts_with("fall") {
        (115.0, 72.0)
    } else if anim.starts_with("jump") {
        (112.0, 68.0)
    } else if anim.starts_with("land") {
        (116.0, 84.0)
    } else {
        (117.0, 82.0)
    }
}
