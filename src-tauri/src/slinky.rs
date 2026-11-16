//! Held by the scruff: Henry's body is a soft chain (a slinky), not a rigid pendulum. Ported from an
//! interactive tuning page (its PHYSICS block): 9 bones of fixed length from the scruff down to
//! the bottom of the art, position-based dynamics with iterations (bones pull each other both ways, so
//! the chain never pumps itself), then two limiters top-down: angle between neighbouring bones and each
//! bone's swing from vertical. Limiters don't add velocity, so nothing jitters against them. The drawing
//! follows a Catmull-Rom spline through the nodes (render.rs `render_slinky`).
//!
//! All tuning is in sprite pixels and seconds (as on the page); the chain lives in screen px, so lengths,
//! gravity and speed caps are multiplied by the sprite scale `k`.

/// Tuned on that page (2026-11-13, its "Параметры для Rust" block).
pub struct Params {
    /// Joint straightening rate at the top / bottom of the chain, 1/s (0 = rope).
    pub stiff_top: f64,
    pub stiff_bottom: f64,
    /// Air drag, 1/s.
    pub damping: f64,
    /// Sprite px/s².
    pub gravity: f64,
    /// Max angle between neighbouring bones.
    pub joint_limit_deg: f64,
    /// Max angle of any bone from vertical.
    pub max_swing_deg: f64,
    /// Scruff -> cursor spring (critically damped), rad/s.
    pub follow: f64,
}

pub const PARAMS: Params =
    Params { stiff_top: 12.0, stiff_bottom: 18.5, damping: 4.5, gravity: 1000.0, joint_limit_deg: 25.0, max_swing_deg: 55.0, follow: 18.0 };

pub const BONES: usize = 9;
const DT: f64 = 1.0 / 120.0;
const ITER: usize = 16;
/// Node speed cap relative to the scruff, sprite px/s. The page capped absolute speed (and the scruff at
/// 450 too); on the desktop she is dragged much faster, so only the swinging part is capped.
const MAX_NODE_V: f64 = 450.0;

#[derive(Clone, Copy, Default)]
struct Node {
    x: f64,
    y: f64,
    px: f64,
    py: f64,
}

pub struct Slinky {
    nodes: [Node; BONES + 1],
    /// bone length, screen px
    len: f64,
    /// sprite scale (screen px per cell px)
    k: f64,
    /// held point and its velocity (screen px, px/s)
    a: (f64, f64),
    av: (f64, f64),
    acc: f64,
    /// grab point in cell px; the chain runs from it to the bottom of the art
    pub pivot: (f64, f64),

}

/// Pull a and c apart/together along their line; wa, wc are inverse masses (0 = pinned).
fn push_pair(n: &mut [Node], a: Option<usize>, ghost: &mut (f64, f64), c: usize, want: f64, k: f64, wa: f64, wc: f64) {
    let (ax, ay) = match a {
        Some(i) => (n[i].x, n[i].y),
        None => *ghost,
    };
    let (dx, dy) = (n[c].x - ax, n[c].y - ay);
    let d = (dx * dx + dy * dy).sqrt().max(1e-6);
    let w = wa + wc;
    if w == 0.0 {
        return;
    }
    let corr = (want - d) * k;
    let (ux, uy) = (dx / d, dy / d);
    if let Some(i) = a {
        n[i].x -= ux * corr * wa / w;
        n[i].y -= uy * corr * wa / w;
    }
    n[c].x += ux * corr * wc / w;
    n[c].y += uy * corr * wc / w;
}

impl Slinky {
    /// Hanging straight down from the scruff at screen `p`; `pivot`/`bottom` in cell px, `k` = scale.
    pub fn new(p: (f64, f64), pivot: (f64, f64), bottom: f64, k: f64) -> Slinky {
        let len = (bottom - pivot.1).max(BONES as f64) / BONES as f64 * k;
        let mut nodes = [Node::default(); BONES + 1];
        for (i, nd) in nodes.iter_mut().enumerate() {
            let y = p.1 + i as f64 * len;
            *nd = Node { x: p.0, y, px: p.0, py: y };
        }
        Slinky { nodes, len, k, a: p, av: (0.0, 0.0), acc: 0.0, pivot }
    }

    /// Scruff position on screen.
    pub fn scruff(&self) -> (f64, f64) {
        (self.nodes[0].x, self.nodes[0].y)
    }

    /// Bone length in screen px.
    pub fn bone(&self) -> f64 {
        self.len
    }

    /// Advance by `dt` s with the scruff pulled toward `target` (screen px). Fixed 120 Hz substeps.
    pub fn step(&mut self, target: (f64, f64), dt: f64) {
        self.acc += dt.min(0.05);
        let mut c = 0;
        while self.acc >= DT && c < 8 {
            self.tick(&PARAMS, target);
            self.acc -= DT;
            c += 1;
        }
        if c == 8 {
            self.acc = 0.0;
        }
    }

    fn tick(&mut self, p: &Params, t: (f64, f64)) {
        let (n, l, k) = (BONES, self.len, self.k);
        let nodes = &mut self.nodes;

        // held point: critically damped spring toward the cursor
        let w = p.follow;
        self.av.0 += (w * w * (t.0 - self.a.0) - 2.0 * w * self.av.0) * DT;
        self.av.1 += (w * w * (t.1 - self.a.1) - 2.0 * w * self.av.1) * DT;
        self.a.0 += self.av.0 * DT;
        self.a.1 += self.av.1 * DT;
        nodes[0].px = nodes[0].x;
        nodes[0].py = nodes[0].y;
        nodes[0].x = self.a.0;
        nodes[0].y = self.a.1;
        // the grip holds the first bone vertical
        let mut ghost = (self.a.0, self.a.1 - l);

        // inertia, drag, gravity
        let drag = (-p.damping * DT).exp();
        let g = p.gravity * k * DT * DT;
        let max_step = MAX_NODE_V * k * DT;
        let (avx, avy) = (self.av.0 * DT, self.av.1 * DT);
        for nd in nodes.iter_mut().skip(1) {
            let (mut vx, mut vy) = ((nd.x - nd.px) * drag, (nd.y - nd.py) * drag);
            let (rx, ry) = (vx - avx, vy - avy);
            let sl = (rx * rx + ry * ry).sqrt();
            if sl > max_step {
                vx = avx + rx * max_step / sl;
                vy = avy + ry * max_step / sl;
            }
            nd.px = nd.x;
            nd.py = nd.y;
            nd.x += vx;
            nd.y += vy + g;
        }

        let k_iter: Vec<f64> = (0..n)
            .map(|i| {
                let rate = p.stiff_top + (p.stiff_bottom - p.stiff_top) * if n > 1 { i as f64 / (n - 1) as f64 } else { 0.0 };
                1.0 - (-rate * DT / ITER as f64).exp()
            })
            .collect();

        for _ in 0..ITER {
            // bone lengths (node 0 is pinned)
            for i in 0..n {
                push_pair(nodes, Some(i), &mut ghost, i + 1, l, 1.0, if i == 0 { 0.0 } else { 1.0 }, 1.0);
            }
            // bending: soft spring pulling every joint toward straight
            for i in 0..n {
                let a = if i == 0 { None } else { Some(i - 1) };
                let wa = if i <= 1 { 0.0 } else { 1.0 };
                push_pair(nodes, a, &mut ghost, i + 1, 2.0 * l, k_iter[i], wa, 1.0);
            }
        }

        // limiters, applied last and top-down so they always hold and bone lengths stay exact
        let (max_joint, max_swing) = (p.joint_limit_deg.to_radians(), p.max_swing_deg.to_radians());
        let (mut pdx, mut pdy) = (0.0f64, 1.0f64);
        for i in 1..=n {
            let (hx, hy) = (nodes[i - 1].x, nodes[i - 1].y);
            let (mut ex, mut ey) = (nodes[i].x - hx, nodes[i].y - hy);
            let el = (ex * ex + ey * ey).sqrt().max(1e-6);
            ex /= el;
            ey /= el;
            let ang = (pdx * ey - pdy * ex).atan2(pdx * ex + pdy * ey).clamp(-max_joint, max_joint);
            let (sa, ca) = ang.sin_cos();
            ex = pdx * ca - pdy * sa;
            ey = pdx * sa + pdy * ca;
            let sw = (-ex).atan2(ey);
            if sw.abs() > max_swing {
                let sw = sw.signum() * max_swing;
                ex = -sw.sin();
                ey = sw.cos();
            }
            let (cx, cy) = (hx + ex * l, hy + ey * l);
            let q = &mut nodes[i];
            q.px += cx - q.x;
            q.py += cy - q.y;
            q.x = cx;
            q.y = cy;
            pdx = ex;
            pdy = ey;
        }
    }

    /// Point and tangent (screen px per bone) on the body line; `u` in bones from the scruff. Above the
    /// scruff (u < 0) the head stays straight up, past the last node the line runs on along the last bone.
    pub fn at(&self, u: f64) -> (f64, f64, f64, f64) {
        let (nd, n, l) = (&self.nodes, BONES, self.len);
        if u <= 0.0 {
            return (nd[0].x, nd[0].y + u * l, 0.0, l);
        }
        if u >= n as f64 {
            let (tx, ty) = (nd[n].x - nd[n - 1].x, nd[n].y - nd[n - 1].y);
            let e = u - n as f64;
            return (nd[n].x + tx * e, nd[n].y + ty * e, tx, ty);
        }
        let i = (u.floor() as usize).min(n - 1);
        let t = u - i as f64;
        let p0 = if i == 0 { (nd[0].x, nd[0].y - l) } else { (nd[i - 1].x, nd[i - 1].y) };
        let p1 = (nd[i].x, nd[i].y);
        let p2 = (nd[i + 1].x, nd[i + 1].y);
        let p3 = if i + 2 <= n { (nd[i + 2].x, nd[i + 2].y) } else { (2.0 * nd[n].x - nd[n - 1].x, 2.0 * nd[n].y - nd[n - 1].y) };
        let (t2, t3) = (t * t, t * t * t);
        let pos = |a: f64, b: f64, c: f64, d: f64| {
            0.5 * (2.0 * b + (c - a) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (3.0 * b - a - 3.0 * c + d) * t3)
        };
        let tan = |a: f64, b: f64, c: f64, d: f64| {
            0.5 * ((c - a) + 2.0 * (2.0 * a - 5.0 * b + 4.0 * c - d) * t + 3.0 * (3.0 * b - a - 3.0 * c + d) * t2)
        };
        (pos(p0.0, p1.0, p2.0, p3.0), pos(p0.1, p1.1, p2.1, p3.1), tan(p0.0, p1.0, p2.0, p3.0), tan(p0.1, p1.1, p2.1, p3.1))
    }
}
