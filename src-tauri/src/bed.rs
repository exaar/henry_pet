//! Henry's bed (2026-11-15, PLAN #51): a separate desktop object, drawn in code in her palette (no
//! art file) and deformed procedurally. A round cat bed seen from the front: back rim, cushion, front rim.
//!
//! - `Jelly`: the soft part. The cushion is a row of damped springs coupled to their neighbours (a 1D wave),
//!   plus one global squash spring for the whole bed. Henry's weight presses columns down, landings kick them.
//! - `draw`: the bed at native pixels from the current deformation (pixel art: one native px = k screen px,
//!   nearest sampling), with the front rim marked so it can go on a window in FRONT of Henry when she lies in it.
//! - `Dangle`: carried in her teeth - a rigid bed pinned at one rim end to her mouth, swinging under gravity,
//!   kept above the surface she walks on (so on the ground it drags).
//! - Held by the user: the bed hangs from the grabbed end on the same soft chain as Henry (slinky.rs).
//! - `View`: two layered windows (back = whole bed, front = front rim while she is inside) + hit testing.
//!
//! - `BedObj` (module `body` below): the bed as a body in her world - motion on the floor / window tops / in the
//!   air / held / carried, shared with Henry's world (brain/world.rs).
use crate::slinky::Slinky;
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, POINT, SIZE};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteObject, GetDC, ReleaseDC, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, RegisterClassW, UpdateLayeredWindow, ULW_ALPHA, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

/// Bed length at rest, native px (Henry's sprawl is ~115 of her 128 px cell).
pub const BW: f64 = 136.0;
/// Drawing buffer, native px: room to widen when squashed and to stretch up.
pub const DW: usize = 150;
pub const DH: usize = 48;
/// Cushion spring columns across the bed.
const NC: usize = 35;
/// Henry's foot line in the bed, native px below the front rim top (her lying art reaches 17 px below the
/// foot line; the rim hides that part, so she lies IN the bed).
const SINK: f64 = 3.0;

// Henry's palette (art/henry/palette.json): mauve rim, pink cushion, her own outline colour.
const OUTLINE: [u8; 3] = [0x20, 0x1e, 0x2c];
const RIM_DARK: [u8; 3] = [0x6b, 0x4a, 0x5d];
const RIM: [u8; 3] = [0xa9, 0x78, 0x8a];
const RIM_LIGHT: [u8; 3] = [0xf6, 0xa7, 0xb9];
const SEAM: [u8; 3] = [0x49, 0x37, 0x47];
const CUSH: [u8; 3] = [0xf9, 0xbf, 0xce];
const CUSH_SHADE: [u8; 3] = [0xf6, 0xa7, 0xb9];
const CUSH_DEEP: [u8; 3] = [0xa9, 0x78, 0x8a];

const DT: f64 = 1.0 / 120.0;
// cushion: per-column spring (1/s²), damping (1/s), neighbour coupling (1/s²)
const K: f64 = 140.0;
const C: f64 = 7.0;
const T: f64 = 650.0;
// whole-bed squash spring
const KQ: f64 = 260.0;
const CQ: f64 = 9.0;

/// The soft part of the bed: cushion sag per column (native px, + = down) and a global squash.
pub struct Jelly {
    s: [f64; NC],
    v: [f64; NC],
    /// static load for this frame (target sag per column), rebuilt every frame by `press`
    load: [f64; NC],
    pub q: f64,
    qv: f64,
    qload: f64,
    acc: f64,
}

fn col_u(i: usize) -> f64 {
    -1.0 + 2.0 * i as f64 / (NC - 1) as f64
}

/// Smooth bump: 1 at the centre, 0 at ±half.
fn bump(u: f64, at: f64, half: f64) -> f64 {
    let d = ((u - at) / half).abs();
    if d >= 1.0 {
        0.0
    } else {
        0.5 + 0.5 * (std::f64::consts::PI * d).cos()
    }
}

impl Jelly {
    pub fn new() -> Jelly {
        Jelly { s: [0.0; NC], v: [0.0; NC], load: [0.0; NC], q: 0.0, qv: 0.0, qload: 0.0, acc: 0.0 }
    }

    /// Weight on the cushion this frame: sag `depth` px under a bump at `at` (bed units -1..1) of `half` width.
    pub fn press(&mut self, at: f64, half: f64, depth: f64) {
        for i in 0..NC {
            self.load[i] += depth * bump(col_u(i), at, half);
        }
        self.qload += depth * 0.004;
    }

    /// A blow: cushion speed `dv` px/s down under the bump, squash speed `dq` /s.
    pub fn kick(&mut self, at: f64, half: f64, dv: f64, dq: f64) {
        for i in 0..NC {
            self.v[i] += dv * bump(col_u(i), at, half);
        }
        self.qv += dq;
    }

    pub fn step(&mut self, dt: f64) {
        self.acc += dt.min(0.05);
        while self.acc >= DT {
            self.acc -= DT;
            let s = self.s;
            for i in 0..NC {
                let l = if i > 0 { s[i - 1] } else { 0.0 };
                let r = if i + 1 < NC { s[i + 1] } else { 0.0 };
                let a = -K * (s[i] - self.load[i]) - C * self.v[i] + T * (l + r - 2.0 * s[i]);
                self.v[i] += a * DT;
            }
            for i in 0..NC {
                self.s[i] = (self.s[i] + self.v[i] * DT).clamp(-5.0, 9.0);
            }
            let aq = -KQ * (self.q - self.qload) - CQ * self.qv;
            self.qv += aq * DT;
            self.q = (self.q + self.qv * DT).clamp(-0.22, 0.42);
        }
        self.load = [0.0; NC];
        self.qload = 0.0;
    }

    fn sag(&self, u: f64) -> f64 {
        let f = ((u + 1.0) / 2.0 * (NC - 1) as f64).clamp(0.0, (NC - 1) as f64);
        let i = (f.floor() as usize).min(NC - 2);
        let t = f - i as f64;
        self.s[i] * (1.0 - t) + self.s[i + 1] * t
    }

    /// Henry's foot line in the bed, native px above the bed bottom.
    pub fn foot(&self) -> f64 {
        self.profile(DW as f64 / 2.0).map_or(17.0, |p| p.hf - SINK)
    }

    /// Height of the walkable top (front rim / cushion) at native buffer x, px above the bottom.
    pub fn top_at(&self, bx: f64) -> Option<f64> {
        self.profile(bx).map(|p| p.hf.max(p.ht * 0.55))
    }

    fn profile(&self, x: f64) -> Option<Prof> {
        let widen = 1.0 + 0.5 * self.q;
        let half = BW / 2.0 * widen;
        let u = (x - DW as f64 / 2.0) / half;
        if u.abs() >= 1.0 {
            return None;
        }
        let e = (1.0 - u * u).sqrt();
        let hs = 1.0 - self.q;
        let sag = self.sag(u);
        // a donut seen from a little above: round ends (the bottom curls up at them), a fat back tube,
        // the cushion between, a lower front tube
        let hb = 4.0 * (1.0 - e.powf(0.35)) * hs;
        let ht = (6.0 + 32.0 * e.powf(0.6)) * hs - sag * 0.3 * e;
        let hf = (6.0 + 15.0 * e.powf(0.7)) * hs - sag * 0.7;
        let hbi = (ht - (9.0 * e.powf(0.6) + 1.0) * hs).max(hf);
        Some(Prof { hb, ht, hf, hbi, sag })
    }

    /// The bed at native px: RGBA (straight, alpha 0/255) and which pixels are front rim.
    pub fn draw(&self) -> (Vec<[u8; 4]>, Vec<bool>) {
        let mut px = vec![[0u8; 4]; DW * DH];
        let mut front = vec![false; DW * DH];
        let put = |px: &mut Vec<[u8; 4]>, i: usize, c: [u8; 3]| px[i] = [c[0], c[1], c[2], 255];
        for x in 0..DW {
            let Some(p) = self.profile(x as f64 + 0.5) else { continue };
            for y in 0..DH {
                let h = (DH - 1 - y) as f64 + 0.5;
                let i = y * DW + x;
                if h < p.hb {
                    continue;
                }
                if h < p.hf {
                    // front rim: lit top, darker toward the floor, a stitched seam along the middle
                    let from_top = p.hf - h;
                    let c = if from_top < 1.0 {
                        OUTLINE
                    } else if from_top < 3.0 {
                        RIM_LIGHT
                    } else if h - p.hb < 3.0 {
                        RIM_DARK
                    } else if (h - (p.hf + p.hb) * 0.5).abs() < 0.5 && x % 6 == 0 {
                        SEAM
                    } else {
                        RIM
                    };
                    put(&mut px, i, c);
                    front[i] = true;
                } else if h < p.hbi {
                    // cushion: shadow under the back rim, darker where it is pressed down
                    let above_front = h - p.hf;
                    let c = if p.hbi - h < 1.5 {
                        CUSH_SHADE
                    } else if p.sag > 3.5 && above_front < 1.0 {
                        CUSH_DEEP
                    } else if p.sag > 1.2 && above_front < p.sag * 0.8 {
                        CUSH_SHADE
                    } else {
                        CUSH
                    };
                    put(&mut px, i, c);
                } else if h < p.ht {
                    let from_top = p.ht - h;
                    let c = if from_top < 2.5 {
                        RIM_LIGHT
                    } else if h - p.hbi < 1.0 && p.hbi > p.hf {
                        RIM_DARK
                    } else {
                        RIM
                    };
                    put(&mut px, i, c);
                }
            }
        }
        // outline: every filled pixel touching empty space
        let filled = |x: i32, y: i32| x >= 0 && y >= 0 && (x as usize) < DW && (y as usize) < DH && px[y as usize * DW + x as usize][3] > 0;
        let mut edge = Vec::new();
        for y in 0..DH as i32 {
            for x in 0..DW as i32 {
                if filled(x, y) && [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| !filled(x + dx, y + dy)) {
                    edge.push(y as usize * DW + x as usize);
                }
            }
        }
        for i in edge {
            px[i] = [OUTLINE[0], OUTLINE[1], OUTLINE[2], 255];
        }
        (px, front)
    }
}

struct Prof {
    /// bottom curl at the round ends
    hb: f64,
    ht: f64,
    hf: f64,
    hbi: f64,
    sag: f64,
}

/// Where Henry holds it: one rim end, native px in the draw buffer. `side` +1 = right end.
pub fn grip(side: i8) -> (f64, f64) {
    let x = DW as f64 / 2.0 + side as f64 * (BW / 2.0 - 14.0);
    (x, DH as f64 - 21.0)
}

/// Carried in Henry's teeth: a rigid bed pinned at one rim end, swinging under gravity, kept above `floor`.
pub struct Dangle {
    /// pin (her mouth, spring-followed) and the bed's centre of mass, screen px; Verlet for the mass
    pin: (f64, f64),
    pin_v: (f64, f64),
    c: (f64, f64),
    cp: (f64, f64),
    pin_local: (f64, f64),
    com_local: (f64, f64),
    len: f64,
    k: f64,
    acc: f64,
}

impl Dangle {
    /// Start from the upright bed with bottom-centre (x, y) on screen, gripped at `side`.
    pub fn new(x: f64, y: f64, k: f64, side: i8) -> Dangle {
        let to_screen = |l: (f64, f64)| (x + (l.0 - DW as f64 / 2.0) * k, y + (l.1 - DH as f64) * k);
        let pin_local = grip(side);
        let com_local = (DW as f64 / 2.0, DH as f64 - 12.0);
        let pin = to_screen(pin_local);
        let c = to_screen(com_local);
        let len = ((com_local.0 - pin_local.0).powi(2) + (com_local.1 - pin_local.1).powi(2)).sqrt() * k;
        Dangle { pin, pin_v: (0.0, 0.0), c, cp: c, pin_local, com_local, len, k, acc: 0.0 }
    }

    /// Rotation of the bed (radians, screen y down).
    pub fn angle(&self) -> f64 {
        let (lx, ly) = (self.com_local.0 - self.pin_local.0, self.com_local.1 - self.pin_local.1);
        (self.c.1 - self.pin.1).atan2(self.c.0 - self.pin.0) - ly.atan2(lx)
    }

    /// Native buffer point -> screen.
    pub fn world(&self, l: (f64, f64)) -> (f64, f64) {
        let a = self.angle();
        let (s, c) = a.sin_cos();
        let (dx, dy) = ((l.0 - self.pin_local.0) * self.k, (l.1 - self.pin_local.1) * self.k);
        (self.pin.0 + dx * c - dy * s, self.pin.1 + dx * s + dy * c)
    }

    pub fn pin(&self) -> (f64, f64) {
        self.pin
    }
    pub fn pin_local(&self) -> (f64, f64) {
        self.pin_local
    }
    pub fn pin_velocity(&self) -> (f64, f64) {
        self.pin_v
    }

    pub fn step(&mut self, mouth: (f64, f64), floor: Option<f64>, dt: f64) {
        self.acc += dt.min(0.05);
        let mut n = 0;
        while self.acc >= DT && n < 8 {
            self.acc -= DT;
            n += 1;
            // pin: stiff critically damped spring to the mouth (art frames jump the mouth a few px)
            let w = 30.0;
            self.pin_v.0 += (w * w * (mouth.0 - self.pin.0) - 2.0 * w * self.pin_v.0) * DT;
            self.pin_v.1 += (w * w * (mouth.1 - self.pin.1) - 2.0 * w * self.pin_v.1) * DT;
            self.pin.0 += self.pin_v.0 * DT;
            self.pin.1 += self.pin_v.1 * DT;
            let drag = (-3.0 * DT).exp();
            let (vx, vy) = ((self.c.0 - self.cp.0) * drag, (self.c.1 - self.cp.1) * drag);
            self.cp = self.c;
            self.c.0 += vx;
            self.c.1 += vy + 1800.0 * self.k * DT * DT;
            for _ in 0..4 {
                self.constrain();
                if let Some(f) = floor {
                    let low = [(DW as f64 / 2.0 - BW / 2.0, DH as f64 - 1.0), (DW as f64 / 2.0 + BW / 2.0, DH as f64 - 1.0), (DW as f64 / 2.0, DH as f64 - 1.0)]
                        .iter()
                        .map(|&p| self.world(p).1)
                        .fold(f64::MIN, f64::max);
                    if low > f {
                        self.c.1 -= low - f;
                    }
                }
            }
            self.constrain();
        }
        if n == 8 {
            self.acc = 0.0;
        }
    }

    fn constrain(&mut self) {
        let (dx, dy) = (self.c.0 - self.pin.0, self.c.1 - self.pin.1);
        let d = (dx * dx + dy * dy).sqrt().max(1e-6);
        self.c = (self.pin.0 + dx / d * self.len, self.pin.1 + dy / d * self.len);
    }
}

/// Held by the user from one end: the bed turned on end (grabbed end up) so the soft chain runs along its length.
/// `left_up`: the left end is on top. Returns native RGBA, its size (w = DH, h = DW) and the grab point.
pub fn held_image(px: &[[u8; 4]], left_up: bool) -> (Vec<[u8; 4]>, (f64, f64), f64) {
    let (w, h) = (DH, DW);
    let mut out = vec![[0u8; 4]; w * h];
    for v in 0..h {
        for u in 0..w {
            let (bx, by) = if left_up { (v, DH - 1 - u) } else { (DW - 1 - v, u) };
            out[v * w + u] = px[by * DW + bx];
        }
    }
    let rows: Vec<usize> = (0..h).filter(|&v| (0..w).any(|u| out[v * w + u][3] > 0)).collect();
    let (first, last) = (rows.first().copied().unwrap_or(0), rows.last().copied().unwrap_or(h - 1));
    let gv = (first + 10).min(last);
    let cols: Vec<usize> = (0..w).filter(|&u| out[gv * w + u][3] > 0).collect();
    let gu = if cols.is_empty() { w as f64 / 2.0 } else { (cols[0] + cols[cols.len() - 1]) as f64 / 2.0 + 0.5 };
    (out, (gu, gv as f64), last as f64 + 1.0)
}

/// Native RGBA -> premultiplied BGRA scaled by integer-free nearest sampling to (w, h).
fn scale(px: &[[u8; 4]], sw: usize, sh: usize, w: i32, h: i32, keep: impl Fn(usize) -> bool) -> Vec<u8> {
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        let sy = ((y as f64 + 0.5) * sh as f64 / h as f64) as usize;
        for x in 0..w {
            let sx = ((x as f64 + 0.5) * sw as f64 / w as f64) as usize;
            let i = sy.min(sh - 1) * sw + sx.min(sw - 1);
            let p = px[i];
            if p[3] > 0 && keep(i) {
                let o = ((y * w + x) * 4) as usize;
                out[o..o + 4].copy_from_slice(&[p[2], p[1], p[0], 255]);
            }
        }
    }
    out
}

/// One layered, click-through-by-default topmost window.
pub struct Layer {
    pub hwnd: isize,
    mem: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    size: (i32, i32),
    /// what is on screen: rect + premultiplied BGRA (for hit tests)
    pub rect: (i32, i32, i32, i32),
    buf: Vec<u8>,
    pub shown: bool,
    interactive: bool,
}

impl Layer {
    fn new(title: windows::core::PCWSTR) -> Layer {
        unsafe {
            let inst = GetModuleHandleW(None).expect("module handle");
            let class = w!("HenryPetBed");
            let wc = WNDCLASSW { lpfnWndProc: Some(crate::render::wndproc), hInstance: inst.into(), lpszClassName: class, ..Default::default() };
            RegisterClassW(&wc); // second call fails harmlessly (class exists)
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class,
                title,
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(inst.into()),
                None,
            )
            .expect("bed window");
            let screen = GetDC(None);
            let mem = CreateCompatibleDC(Some(screen));
            ReleaseDC(None, screen);
            Layer {
                hwnd: hwnd.0 as isize,
                mem,
                bmp: HBITMAP::default(),
                old: HGDIOBJ::default(),
                bits: std::ptr::null_mut(),
                size: (0, 0),
                rect: (0, 0, 0, 0),
                buf: Vec::new(),
                shown: false,
                interactive: false,
            }
        }
    }

    fn present(&mut self, x: i32, y: i32, w: i32, h: i32, buf: Vec<u8>) {
        if self.rect == (x, y, w, h) && self.buf == buf && self.shown {
            return;
        }
        unsafe {
            if self.size != (w, h) {
                if !self.bmp.is_invalid() {
                    SelectObject(self.mem, self.old);
                    let _ = DeleteObject(self.bmp.into());
                }
                let bi = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: w,
                        biHeight: -h,
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB.0,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
                self.bmp = CreateDIBSection(Some(self.mem), &bi, DIB_RGB_COLORS, &mut bits, None, 0).expect("dib");
                self.old = SelectObject(self.mem, self.bmp.into());
                self.bits = bits as *mut u8;
                self.size = (w, h);
            }
            std::slice::from_raw_parts_mut(self.bits, (w * h * 4) as usize).copy_from_slice(&buf);
            let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
            let _ = UpdateLayeredWindow(
                HWND(self.hwnd as *mut _),
                None,
                Some(&POINT { x, y }),
                Some(&SIZE { cx: w, cy: h }),
                Some(self.mem),
                Some(&POINT { x: 0, y: 0 }),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
        self.rect = (x, y, w, h);
        self.buf = buf;
        self.show(true);
    }

    fn show(&mut self, on: bool) {
        if on != self.shown {
            self.shown = on;
            crate::win::set_visible(self.hwnd, on);
        }
    }

    fn hit(&self, cx: i32, cy: i32) -> bool {
        let (x, y, w, h) = self.rect;
        if !self.shown || cx < x || cy < y || cx >= x + w || cy >= y + h {
            return false;
        }
        let o = (((cy - y) * w + (cx - x)) * 4 + 3) as usize;
        self.buf.get(o).is_some_and(|a| *a > 0)
    }

    fn set_interactive(&mut self, on: bool) {
        if on != self.interactive {
            self.interactive = on;
            crate::win::set_click_through(self.hwnd, !on);
        }
    }
}

/// The bed on screen: back layer (under Henry) and front rim layer (over her, only while she is inside).
pub struct View {
    pub back: Layer,
    pub front: Layer,
}

impl View {
    pub fn new() -> View {
        View { back: Layer::new(w!("Henry bed")), front: Layer::new(w!("Henry bed rim")) }
    }

    pub fn hide(&mut self) {
        self.back.show(false);
        self.front.show(false);
    }

    /// Upright on a surface: bottom-centre at screen (x, y); `split` puts the front rim on the front layer.
    /// `paw`: screen pixels where Henry's paw hangs over the front rim (kept out of the front layer).
    pub fn upright(&mut self, j: &Jelly, x: f64, y: f64, k: f64, split: bool, paw: &dyn Fn(i32, i32) -> bool) {
        let (px, front) = j.draw();
        let (w, h) = ((DW as f64 * k).round() as i32, (DH as f64 * k).round() as i32);
        let (wx, wy) = ((x - w as f64 / 2.0).round() as i32, (y - h as f64).round() as i32);
        if split {
            let b = scale(&px, DW, DH, w, h, |i| !front[i]);
            let mut f = scale(&px, DW, DH, w, h, |i| front[i]);
            for yy in 0..h {
                for xx in 0..w {
                    let o = ((yy * w + xx) * 4) as usize;
                    if f[o + 3] > 0 && paw(wx + xx, wy + yy) {
                        f[o..o + 4].copy_from_slice(&[0, 0, 0, 0]);
                    }
                }
            }
            self.back.present(wx, wy, w, h, b);
            self.front.present(wx, wy, w, h, f);
        } else {
            self.back.present(wx, wy, w, h, scale(&px, DW, DH, w, h, |_| true));
            self.front.show(false);
        }
    }

    /// Carried: rotated rigid bed (nearest sampling at screen resolution).
    pub fn rotated(&mut self, j: &Jelly, d: &Dangle, k: f64) {
        let (px, _) = j.draw();
        let pin = d.pin();
        let pl = d.pin_local();
        let reach = [(0.0, 0.0), (DW as f64, 0.0), (0.0, DH as f64), (DW as f64, DH as f64)]
            .iter()
            .map(|(x, y)| ((x - pl.0).powi(2) + (y - pl.1).powi(2)).sqrt())
            .fold(0.0, f64::max)
            * k;
        let s = (reach * 2.0).ceil() as i32 + 4;
        let (ox, oy) = ((pin.0 - s as f64 / 2.0).round() as i32, (pin.1 - s as f64 / 2.0).round() as i32);
        let (sn, cs) = d.angle().sin_cos();
        let mut out = vec![0u8; (s * s * 4) as usize];
        for yy in 0..s {
            for xx in 0..s {
                let (dx, dy) = ((ox + xx) as f64 + 0.5 - pin.0, (oy + yy) as f64 + 0.5 - pin.1);
                // inverse rotation into native buffer px
                let lx = (dx * cs + dy * sn) / k + pl.0;
                let ly = (-dx * sn + dy * cs) / k + pl.1;
                if lx < 0.0 || ly < 0.0 || lx >= DW as f64 || ly >= DH as f64 {
                    continue;
                }
                let p = px[ly as usize * DW + lx as usize];
                if p[3] > 0 {
                    let o = ((yy * s + xx) * 4) as usize;
                    out[o..o + 4].copy_from_slice(&[p[2], p[1], p[0], 255]);
                }
            }
        }
        self.back.present(ox, oy, s, s, out);
        self.front.show(false);
    }

    /// Held by the user: hanging from one end on the soft chain.
    pub fn held(&mut self, j: &Jelly, body: &Slinky, left_up: bool, k: f64) {
        let (px, _) = j.draw();
        let (img, _, _) = held_image(&px, left_up);
        let (w, h) = ((DH as f64 * k).round() as i32, (DW as f64 * k).round() as i32);
        let cell = scale(&img, DH, DW, w, h, |_| true);
        let (pvx, pvy) = (body.pivot.0 * k, body.pivot.1 * k);
        let reach = [(0.0, 0.0), (w as f64, 0.0), (0.0, h as f64), (w as f64, h as f64)]
            .iter()
            .map(|(x, y)| ((x - pvx).powi(2) + (y - pvy).powi(2)).sqrt())
            .fold(0.0, f64::max);
        let s = (reach * 2.0).ceil() as i32 + 4;
        let (sx, sy) = body.scruff();
        let (x, y) = ((sx - s as f64 / 2.0).round() as i32, (sy - s as f64 / 2.0).round() as i32);
        let mut out = vec![0u8; (s * s * 4) as usize];
        crate::render::slinky_into(&mut out, s, (x, y), &cell, w, h, DW as f64, k, false, body);
        self.back.present(x, y, s, s, out);
        self.front.show(false);
    }

    pub fn hit(&self, cx: i32, cy: i32) -> bool {
        self.back.hit(cx, cy) || self.front.hit(cx, cy)
    }

    pub fn set_interactive(&mut self, on: bool) {
        self.back.set_interactive(on);
        self.front.set_interactive(on);
    }

    /// Back layer right under Henry, front rim right over her (all in the topmost band).
    pub fn order(&self, henry: isize) {
        if self.back.shown {
            crate::win::below(self.back.hwnd, henry);
        }
        if self.front.shown {
            crate::win::topmost(self.front.hwnd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn save(px: &[[u8; 4]], w: usize, h: usize, name: &str, z: u32) {
        let mut img = image::RgbaImage::new(w as u32 * z, h as u32 * z);
        for (x, y, p) in img.enumerate_pixels_mut() {
            let q = px[(y / z) as usize * w + (x / z) as usize];
            *p = if q[3] > 0 { image::Rgba(q) } else { image::Rgba([235, 235, 235, 255]) };
        }
        let dir = std::env::var("BED_PREVIEW").unwrap_or_else(|_| std::env::temp_dir().display().to_string());
        img.save(format!("{dir}/{name}.png")).unwrap();
    }

    /// Preview frames: rest, Henry lying in it, a landing kick a few frames later, held image.
    #[test]
    fn preview() {
        let mut j = Jelly::new();
        save(&j.draw().0, DW, DH, "bed_rest", 4);
        for _ in 0..240 {
            j.press(0.0, 0.6, 3.0);
            j.step(1.0 / 60.0);
        }
        save(&j.draw().0, DW, DH, "bed_lying", 4);
        let (px, front) = j.draw();
        let fr: Vec<[u8; 4]> = px.iter().zip(&front).map(|(p, f)| if *f { *p } else { [0; 4] }).collect();
        save(&fr, DW, DH, "bed_lying_front", 1);
        save(&px, DW, DH, "bed_lying_1x", 1);
        std::fs::write(format!("{}/foot.txt", std::env::var("BED_PREVIEW").unwrap_or_else(|_| std::env::temp_dir().display().to_string())), j.foot().to_string()).unwrap();
        let mut j = Jelly::new();
        j.kick(0.0, 0.5, 70.0, 4.0);
        for i in 0..12 {
            j.press(0.0, 0.6, 3.0);
            j.step(1.0 / 60.0);
            if i % 4 == 3 {
                save(&j.draw().0, DW, DH, &format!("bed_kick{i}"), 4);
            }
        }
        let (img, g, b) = held_image(&Jelly::new().draw().0, true);
        assert!(b > 100.0 && g.1 < 30.0, "held grip {g:?} bottom {b}");
        save(&img, DH, DW, "bed_held", 4);
        assert!(Jelly::new().foot() > 12.0);
    }
}

/// The bed as a body in Henry's world (was in brain.rs).
mod body {
    use crate::brain::*;
    
    use crate::win::{Monitor, Rect};
    
    
    use serde_json::json;
    use std::collections::VecDeque;
    use std::path::PathBuf;
    
    use std::time::{Duration, Instant};
    
    

    #[derive(Clone, Copy, PartialEq, Debug)]
    pub(crate) enum BedMode {
        Ground(Surface),
        Air,
        /// The user holds it by one end (left end up?).
        Held(bool),
        /// In Henry's teeth.
        Carried,
    }

    /// Her bed as a body in the world (PLAN #51). Drawing and the soft parts are in bed.rs.
    pub(crate) struct BedObj {
        pub(crate) view: crate::bed::View,
        pub(crate) jelly: crate::bed::Jelly,
        /// bottom-centre, screen px
        pub(crate) x: f64,
        pub(crate) y: f64,
        pub(crate) vx: f64,
        pub(crate) vy: f64,
        pub(crate) mode: BedMode,
        pub(crate) on_rect: Option<Rect>,
        pub(crate) swing: Option<crate::slinky::Slinky>,
        pub(crate) dangle: Option<crate::bed::Dangle>,
        pub(crate) samples: VecDeque<(Instant, f64, f64)>,
        pub(crate) path: PathBuf,
        /// settled somewhere new: write bed.json
        pub(crate) save: bool,
    }

    impl BedObj {
        pub(crate) fn load(path: PathBuf, mons: &[Monitor]) -> BedObj {
            let saved = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
            let pos = saved.and_then(|v| Some((v.get("x")?.as_f64()?, v.get("y")?.as_f64()?)));
            let p = mons.iter().find(|m| m.primary).or(mons.first()).expect("a monitor");
            let (x, y) = match pos {
                Some((x, y)) if monitor_at(mons, x, y - 2.0).is_some() => (x, y - 4.0),
                _ => (p.work.right as f64 - 260.0, p.work.bottom as f64 - 4.0),
            };
            // starts in the air a few px up: lands on whatever is under it (window top or taskbar)
            BedObj {
                view: crate::bed::View::new(),
                jelly: crate::bed::Jelly::new(),
                x,
                y,
                vx: 0.0,
                vy: 0.0,
                mode: BedMode::Air,
                on_rect: None,
                swing: None,
                dangle: None,
                samples: VecDeque::new(),
                path,
                save: false,
            }
        }

        pub(crate) fn grounded(&self) -> bool {
            matches!(self.mode, BedMode::Ground(_))
        }

        /// Henry's feet when she lies in it (x for her facing `face`, k = scale).
        pub(crate) fn foot(&self, k: f64, face: i8) -> (f64, f64) {
            // her lying art is centred 6.5 px left of her cell centre (facing right)
            (self.x + face as f64 * 6.5 * k, self.y - self.jelly.foot() * k)
        }

        /// Bed units (-1..1) of a screen x.
        pub(crate) fn u(&self, sx: f64, k: f64) -> f64 {
            (sx - self.x) / (crate::bed::BW / 2.0 * k)
        }

        /// Lift for Henry walking over it on the same surface (screen px), if she is over it.
        pub(crate) fn lift(&self, hx: f64, hy: f64, k: f64) -> Option<f64> {
            if !self.grounded() || (hy - self.y).abs() > 2.5 {
                return None;
            }
            let bx = (hx - self.x) / k + crate::bed::DW as f64 / 2.0;
            self.jelly.top_at(bx).map(|t| (t - 1.0) * k)
        }

        pub(crate) fn grab(&mut self, cx: f64, cy: f64, k: f64) {
            let left_up = cx < self.x;
            let (px, _) = self.jelly.draw();
            let (_, (gu, gv), bottom) = crate::bed::held_image(&px, left_up);
            // where that grip point is on the upright bed now -> glides to the cursor
            let (lx, ly) = if left_up { (gv, crate::bed::DH as f64 - 1.0 - gu) } else { (crate::bed::DW as f64 - 1.0 - gv, gu) };
            let p = (self.x + (lx - crate::bed::DW as f64 / 2.0) * k, self.y + (ly - crate::bed::DH as f64) * k);
            self.swing = Some(crate::slinky::Slinky::new(p, (gu, gv), bottom, k));
            self.mode = BedMode::Held(left_up);
            self.dangle = None;
            self.samples.clear();
            let _ = cy;
        }

        /// Henry bites one end (`side` +1 = right end) and lifts it.
        pub(crate) fn pick_up(&mut self, k: f64, side: i8) {
            self.dangle = Some(crate::bed::Dangle::new(self.x, self.y, k, side));
            self.mode = BedMode::Carried;
            self.jelly.kick(side as f64 * 0.7, 0.4, -40.0, -1.0);
        }

        /// Let go from teeth or hand: upright again where its bottom centre is, falling.
        pub(crate) fn drop_it(&mut self, vx: f64, vy: f64) {
            if let Some(d) = self.dangle.take() {
                let (bx, by) = d.world((crate::bed::DW as f64 / 2.0, crate::bed::DH as f64 - 1.0));
                self.x = bx;
                self.y = by;
            }
            self.swing = None;
            self.vx = vx;
            self.vy = vy;
            self.mode = BedMode::Air;
        }

        /// One frame of motion. `mouth`/`floor`: where Henry's mouth is and the surface under her (carried).
        #[allow(clippy::too_many_arguments)]
        pub(crate) fn step(&mut self, world: &World, k: f64, dt: f64, now: Instant, cursor: (f64, f64), lbutton: bool, mouth: (f64, f64), floor: Option<f64>, log: &mut Vec<String>) {
            let half = crate::bed::BW / 2.0 * k;
            match self.mode {
                BedMode::Held(left_up) => {
                    let (cx, cy) = cursor;
                    if let Some(sw) = self.swing.as_mut() {
                        sw.step((cx, cy), dt);
                        let (sx, sy) = sw.scruff();
                        // upright stand-in position (what it falls as when let go): around the middle of the hang
                        self.x = sx;
                        self.y = sy + half;
                    }
                    self.samples.push_back((now, cx, cy));
                    while self.samples.len() > 2 && now - self.samples[0].0 > Duration::from_millis(90) {
                        self.samples.pop_front();
                    }
                    if !lbutton {
                        let (t0, x0, y0) = self.samples[0];
                        let span = (now - t0).as_secs_f64().max(0.016);
                        let v = (((cx - x0) / span).clamp(-4000.0, 4000.0), ((cy - y0) / span).clamp(-4000.0, 4000.0));
                        let _ = left_up;
                        self.drop_it(v.0, v.1);
                        self.jelly.kick(0.0, 1.0, -60.0, -2.0);
                        log.push(format!("bed thrown v=({:.0},{:.0})", v.0, v.1));
                    }
                }
                BedMode::Carried => {
                    if let Some(d) = self.dangle.as_mut() {
                        d.step(mouth, floor, dt);
                        let (bx, by) = d.world((crate::bed::DW as f64 / 2.0, crate::bed::DH as f64 - 1.0));
                        self.x = bx;
                        self.y = by;
                        (self.vx, self.vy) = d.pin_velocity();
                    } else {
                        self.mode = BedMode::Air;
                    }
                }
                BedMode::Air => {
                    self.vy += G * dt;
                    let mut nx = self.x + self.vx * dt;
                    let mut ny = self.y + self.vy * dt;
                    if monitor_at(&world.mons, nx, ny.min(self.y)).is_none() {
                        self.vx = -self.vx * 0.45;
                        nx = self.x;
                    }
                    if let Some(m) = monitor_at(&world.mons, nx, ny) {
                        let ceil = m.rect.top as f64 + crate::bed::DH as f64 * k;
                        if ny < ceil {
                            ny = ceil;
                            self.vy = self.vy.max(0.0);
                        }
                        let (l, r) = (m.rect.left as f64 + half, m.rect.right as f64 - half);
                        let neighbour = |px: f64| world.mons.iter().any(|o| o.handle != m.handle && inside(&o.rect, px, ny.min(o.rect.bottom as f64 - 1.0)));
                        if nx < l && !neighbour(m.rect.left as f64 - 2.0) {
                            nx = l;
                            self.vx = self.vx.abs() * 0.45;
                        } else if nx > r && !neighbour(m.rect.right as f64 + 1.0) {
                            nx = r;
                            self.vx = -self.vx.abs() * 0.45;
                        }
                    }
                    if self.vy > 0.0 {
                        if let Some((s, sy)) = world.landing(nx, self.y, ny) {
                            ny = sy;
                            let impact = self.vy;
                            // soft: a hard landing squashes it and it bounces a little
                            self.jelly.kick(((nx - self.x) / half).clamp(-0.5, 0.5), 0.9, (impact * 0.03).min(80.0), (impact * 0.0018).min(4.0));
                            if impact > 900.0 {
                                self.vy = -impact * 0.22;
                                self.vx *= 0.6;
                            } else {
                                self.mode = BedMode::Ground(s);
                                self.on_rect = match s {
                                    Surface::Window(hw) => world.win(hw).map(|(_, wi)| wi.rect),
                                    _ => None,
                                };
                                (self.vx, self.vy) = (0.0, 0.0);
                                self.save = true;
                            }
                        }
                    }
                    if monitor_at(&world.mons, nx, ny - 2.0).is_none() && monitor_at(&world.mons, nx, self.y).is_none() {
                        if let Some(p) = world.mons.iter().find(|m| m.primary).or(world.mons.first()) {
                            nx = (p.work.left + p.work.right) as f64 / 2.0;
                            ny = p.work.bottom as f64;
                            self.mode = BedMode::Ground(Surface::Floor);
                            (self.vx, self.vy) = (0.0, 0.0);
                        }
                    }
                    self.x = nx;
                    self.y = ny;
                }
                BedMode::Ground(Surface::Window(hw)) => {
                    let fall = match world.win(hw) {
                        Some((i, wi)) => {
                            if let Some(old) = self.on_rect {
                                self.x += (wi.rect.left - old.left) as f64;
                            }
                            self.y = wi.rect.top as f64;
                            self.on_rect = Some(wi.rect);
                            !world.standable(i, self.x)
                        }
                        None => true,
                    };
                    if fall {
                        self.mode = BedMode::Air;
                        (self.vx, self.vy) = (0.0, 0.0);
                        log.push("bed: window gone -> falling".into());
                    }
                }
                BedMode::Ground(_) => match monitor_at(&world.mons, self.x, self.y - 2.0) {
                    Some(m) => {
                        self.y = m.work.bottom as f64;
                        self.x = self.x.clamp(m.work.left as f64 + half * 0.5, m.work.right as f64 - half * 0.5);
                    }
                    None => self.mode = BedMode::Air,
                },
            }
            self.jelly.step(dt);
            if self.save && self.grounded() {
                self.save = false;
                let _ = std::fs::write(&self.path, json!({"x": self.x.round(), "y": self.y.round()}).to_string());
            }
        }

        pub(crate) fn status(&self) -> serde_json::Value {
            let mode = match self.mode {
                BedMode::Ground(Surface::Window(_)) => "window",
                BedMode::Ground(_) => "floor",
                BedMode::Air => "air",
                BedMode::Held(_) => "held",
                BedMode::Carried => "carried",
            };
            json!({"mode": mode, "x": self.x.round(), "y": self.y.round(), "squash": (self.jelly.q * 100.0).round() / 100.0})
        }
    }

    /// Which way she can drag the bed: she stands behind its near end (side -d, ~107 art px out from its
    /// centre) and walks toward d, so she needs room for herself behind it and room ahead. More room first.
    pub(crate) fn drag_dir(world: &World, bed: &BedObj, k: f64, half_body: f64) -> Option<i8> {
        let m = monitor_at(&world.mons, bed.x, bed.y - 2.0)?;
        let (l, r) = (m.work.left as f64, m.work.right as f64);
        let behind = 107.0 * k + half_body + 10.0;
        let ahead = crate::bed::BW / 2.0 * k + 120.0;
        let mut dirs = [1i8, -1];
        if bed.x - l > r - bed.x {
            dirs = [-1, 1];
        }
        dirs.into_iter().find(|&d| {
            let (back_room, front_room) = if d > 0 { (bed.x - l, r - bed.x) } else { (r - bed.x, bed.x - l) };
            back_room > behind && front_room > ahead
        })
    }
}
pub(crate) use body::*;
