//! Her poses on the ground (stand / sit / lie) and the clips that carry her from one to another.
//!
//! Every animation carries a `pose` (pet.json). When the animation she is about to draw is in another
//! pose than the one she is in, a ROUTE plays first (she holds still, the act gets the extra time),
//! then the animation. Without a pose change a LEAD-IN can still play first: the first step from a
//! standstill into the walk cycle. Both are data: new transition art = a new row in a table, not a new
//! branch in the brain. A route or lead-in whose clip the sheet doesn't have is skipped (she pops).
//!
//! The rest of PoseState is the drawing bookkeeping that has to survive between ticks: the walk cycle
//! driven by distance, the paw pin, animation epochs.
use super::*;
use crate::render::{in_group, Pick, Pose, Sheet};
use rand::Rng;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// A clip in a route: one animation by name, or one of its variants at random (`name` or `name_*`).
#[derive(Clone, Copy)]
pub(crate) enum Clip {
    One(&'static str),
    AnyOf(&'static str),
}

/// From one pose to another on the ground: these clips, in order.
pub(crate) struct Route {
    pub(crate) from: Pose,
    pub(crate) to: Pose,
    pub(crate) via: &'static [Clip],
}

pub(crate) const ROUTES: &[Route] = &[
    // get-up from lying has variants (grumpy, yawning): one at random each time
    Route { from: Pose::Lie, to: Pose::Sit, via: &[Clip::AnyOf("trans_unlie")] },
    Route { from: Pose::Lie, to: Pose::Stand, via: &[Clip::AnyOf("trans_unlie"), Clip::One("trans_stand")] },
    Route { from: Pose::Stand, to: Pose::Sit, via: &[Clip::One("trans_sit")] },
    Route { from: Pose::Sit, to: Pose::Stand, via: &[Clip::One("trans_stand")] },
    Route { from: Pose::Sit, to: Pose::Lie, via: &[Clip::One("trans_lie")] },
    Route { from: Pose::Stand, to: Pose::Lie, via: &[Clip::One("trans_sit"), Clip::One("trans_lie")] },
];

/// Starting animation group `into` in `pose` (no pose change) right after one of the groups `after` was
/// drawn: play `via` first. Only when `into` is drawn with her own art, not a stand-in.
pub(crate) struct LeadIn {
    pub(crate) pose: Pose,
    pub(crate) into: &'static str,
    pub(crate) after: &'static [&'static str],
    pub(crate) via: Clip,
}

pub(crate) const LEAD_INS: &[LeadIn] = &[
    // setting off from standing (stand, or just got up): the first step into the walk cycle.
    // Not after a landing / a stand-in trot (zoomies bolt off at once).
    LeadIn { pose: Pose::Stand, into: "walk", after: &["stand", "trans_stand"], via: Clip::One("trans_walk") },
];

impl Clip {
    fn resolve(self, sheet: &Sheet, rng: &mut impl Rng) -> Option<usize> {
        match self {
            Clip::One(name) => sheet.find(name),
            Clip::AnyOf(group) => {
                let v: Vec<usize> = (0..sheet.anims.len())
                    .filter(|&i| {
                        let n = &sheet.anims[i].name;
                        n == group || n.strip_prefix(group).is_some_and(|r| r.starts_with('_'))
                    })
                    .collect();
                if v.is_empty() {
                    None
                } else {
                    Some(v[rng.gen_range(0..v.len())])
                }
            }
        }
    }
}

/// All clips of a route, or none if one is missing (every clip is resolved, in order, either way).
fn resolve_all(via: &[Clip], sheet: &Sheet, rng: &mut impl Rng) -> Option<Vec<usize>> {
    let got: Vec<Option<usize>> = via.iter().map(|c| c.resolve(sheet, rng)).collect();
    got.into_iter().collect()
}

/// Pose and animation bookkeeping.
pub(crate) struct PoseState {
    /// body pose
    pub(crate) pose: Pose,
    /// the transition clip playing (anim index, until) and the ones queued behind it
    pub(crate) trans: Option<(usize, Instant)>,
    pub(crate) trans_next: VecDeque<usize>,
    /// travelling transition (trans_walk): sprite px of its travel already applied, direction it walks off in
    pub(crate) trans_moved: f64,
    pub(crate) trans_dir: i8,
    /// walk cycle driven by distance (anim with steps): sprite px into the cycle, last drawn anim, x last tick
    pub(crate) walk_dist: f64,
    pub(crate) last_drawn: usize,
    pub(crate) x_prev: f64,
    pub(crate) last_pin: f64,
    pub(crate) epoch: u64,
    pub(crate) beat_epoch: u64,
    pub(crate) epoch_act: Act,
    pub(crate) epoch_mode: &'static str,
    pub(crate) twitch_until: Option<Instant>,
}

/// What a tick asks of the pose machine.
pub(crate) struct PoseIn<'a> {
    pub(crate) sheet: &'a Sheet,
    pub(crate) now: Instant,
    /// on the ground now (not the frame-start value: a launch this tick already took her off)
    pub(crate) grounded: bool,
    /// direction she faces this tick
    pub(crate) dir: i8,
    /// where she stood before this tick's motion: a clip picked now starts from there
    pub(crate) x_tick0: f64,
    /// sprite scale
    pub(crate) k: f64,
}

impl PoseState {
    pub(crate) fn new(x: f64) -> PoseState {
        PoseState {
            pose: Pose::Stand,
            trans: None,
            trans_next: VecDeque::new(),
            trans_moved: 0.0,
            trans_dir: 1,
            walk_dist: 0.0,
            last_drawn: usize::MAX,
            x_prev: x,
            last_pin: 0.0,
            epoch: 1,
            beat_epoch: 0,
            epoch_act: Act::Idle,
            epoch_mode: "",
            twitch_until: None,
        }
    }

    /// A transition is playing or queued: she holds still.
    pub(crate) fn busy(&self) -> bool {
        self.trans.is_some() || !self.trans_next.is_empty()
    }

    /// Drop transitions (skin switch: their anim indices belonged to the old sheet).
    pub(crate) fn drop_clips(&mut self) {
        self.trans = None;
        self.trans_next.clear();
    }

    /// Time left of the transition under way, queue included (None if none is playing now).
    pub(crate) fn clips_left(&self, now: Instant, sheet: &Sheet) -> Option<Duration> {
        let (_, t_end) = self.trans.filter(|&(_, t)| t > now)?;
        Some(self.trans_next.iter().fold(t_end - now, |d, &n| d + Duration::from_millis(sheet.anims[n].ms as u64)))
    }

    /// One tick: end / chain clips, start a route or lead-in when the pose of `sel` (what the act wants
    /// drawn) differs from hers, and while a clip plays draw it instead of `sel`. A travelling clip
    /// (trans_walk) moves `x` as its frames appear, paw locked on the floor. A started clip adds its
    /// length to `act_until` (the act itself still gets its full time).
    pub(crate) fn step(&mut self, p: &PoseIn, sel: &mut Pick, x: &mut f64, act_until: &mut Instant, rng: &mut impl Rng) {
        let sheet = p.sheet;
        let ms = |a: usize| Duration::from_millis(sheet.anims[a].ms as u64);
        // move her to `upto` sprite px of the clip's travel (None = all of it, handover value included)
        let travel_to = |trans: Option<(usize, Instant)>, upto: Option<f64>, d: i8, moved: &mut f64, x: &mut f64| {
            if let Some((t, _)) = trans {
                let target = upto.unwrap_or_else(|| sheet.anims[t].travel.iter().sum());
                *x += d as f64 * (target - *moved) * p.k;
                *moved = target;
            }
        };
        if self.trans.is_some_and(|(_, t)| t <= p.now) {
            if p.grounded {
                travel_to(self.trans, None, self.trans_dir, &mut self.trans_moved, x);
            }
            self.trans = self.trans_next.pop_front().map(|t| (t, p.now + ms(t)));
            self.trans_moved = 0.0;
        }
        if !p.grounded {
            self.drop_clips();
            self.pose = Pose::Stand;
        } else if self.trans.is_none() {
            // the pose of whatever is about to be drawn - stand-ins included (a "look" drawn with a
            // sitting idle is sitting too; ignoring that let her pop into a sit with no sit-down)
            let want = sheet.anims[sel.anim].pose;
            let clips = if self.pose != want {
                ROUTES.iter().find(|r| r.from == self.pose && r.to == want).and_then(|r| resolve_all(r.via, sheet, rng))
            } else {
                let last = (self.last_drawn < sheet.anims.len()).then(|| sheet.anims[self.last_drawn].name.as_str());
                LEAD_INS
                    .iter()
                    .find(|l| {
                        l.pose == want
                            && !sel.fallback
                            && in_group(&sheet.anims[sel.anim].name, l.into)
                            && last.is_some_and(|n| l.after.iter().any(|g| in_group(n, g)))
                    })
                    .and_then(|l| resolve_all(&[l.via], sheet, rng))
            };
            if let Some(clips) = clips {
                *x = p.x_tick0;
                let first = clips[0];
                self.trans = Some((first, p.now + ms(first)));
                self.trans_moved = 0.0;
                self.trans_dir = p.dir;
                self.trans_next = clips[1..].iter().copied().collect();
                *act_until += clips[1..].iter().fold(ms(first), |d, &n| d + ms(n));
            }
            self.pose = want;
        }
        if let Some((t, t_end)) = self.trans {
            let a = &sheet.anims[t];
            // a travelling one keeps the direction it set off in; the in-place ones follow `dir`
            let d = if a.travel.is_empty() { p.dir } else { self.trans_dir };
            *sel = Pick { anim: t, mirror: a.facing != 0 && a.facing != d, fallback: false };
            if !a.travel.is_empty() {
                let left = t_end.saturating_duration_since(p.now).as_secs_f64() * 1000.0;
                let played = (a.ms as f64 - left).max(0.0);
                travel_to(self.trans, Some(travel_at(&a.seq, &a.travel, a.ms as f64, played)), self.trans_dir, &mut self.trans_moved, x);
            }
        }
    }
}

/// Sprite px of a travelling clip's travel covered `played` ms in, moving smoothly. `travel[i]` is where
/// played entry i lines up with the floor (first value = alignment to the pose before, applied at once;
/// the extra last value = handover to the walk). Between the drawings (runs of one repeated frame) the
/// body slides on linearly every refresh, so she accelerates into the walk instead of hopping a step per
/// drawing (~12 fps).
pub(crate) fn travel_at(seq: &[u32], travel: &[f64], ms: f64, played: f64) -> f64 {
    let n = seq.len();
    if n == 0 || travel.is_empty() || ms <= 0.0 {
        return travel.iter().sum();
    }
    let fd = ms / n as f64;
    let sum_to = |i: usize| travel[..=i.min(travel.len() - 1)].iter().sum::<f64>();
    // knots: (time, travel) at the start of every drawing, then the end of the clip
    let mut knots: Vec<(f64, f64)> = (0..n).filter(|&i| i == 0 || seq[i] != seq[i - 1]).map(|i| (i as f64 * fd, sum_to(i))).collect();
    knots.push((ms, travel.iter().sum()));
    for w in knots.windows(2) {
        let ((t0, v0), (t1, v1)) = (w[0], w[1]);
        if played < t1 {
            return v0 + (v1 - v0) * ((played - t0) / (t1 - t0).max(1e-9)).clamp(0.0, 1.0);
        }
    }
    knots.last().unwrap().1
}
