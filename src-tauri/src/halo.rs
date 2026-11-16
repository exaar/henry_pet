//! Petting halo (PLAN #57): while the user strokes her, a soft pink glow breathes around Henry's silhouette.
//! Own layered click-through window right below hers (so her body covers its inside), mask = a blur of
//! exactly what her window shows now (any frame, mirrored, rotated, held). The glow image is rebuilt only
//! when her picture changes; the breathing is the window's constant alpha, so a frame costs one
//! UpdateLayeredWindow.
use std::time::Instant;
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, POINT, SIZE};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteObject, GetDC, ReleaseDC, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, RegisterClassW, UpdateLayeredWindow, ULW_ALPHA, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

/// Glow colour: a warmer, more saturated take on her pink (palette `f6a7b9`) so it reads on any wallpaper.
const GLOW: [u8; 3] = [0xff, 0x8f, 0xb4];
/// Seconds per breath while she is being petted (a slow purr rhythm).
const BREATH: f64 = 1.6;

pub struct Halo {
    hwnd: isize,
    mem: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    size: (i32, i32),
    /// (Henry's buffer generation, her window size) the glow image was built from
    built: Option<(u64, (i32, i32))>,
    pad: i32,
    shown: bool,
    /// 0 off .. 1 fully on (eases in and out)
    level: f64,
    t0: Instant,
}

impl Halo {
    pub fn new() -> Halo {
        unsafe {
            let inst = GetModuleHandleW(None).expect("module handle");
            let class = w!("HenryPetHalo");
            let wc = WNDCLASSW { lpfnWndProc: Some(crate::render::wndproc), hInstance: inst.into(), lpszClassName: class, ..Default::default() };
            RegisterClassW(&wc);
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class,
                w!("Henry halo"),
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
            .expect("halo window");
            let screen = GetDC(None);
            let mem = CreateCompatibleDC(Some(screen));
            ReleaseDC(None, screen);
            Halo {
                hwnd: hwnd.0 as isize,
                mem,
                bmp: HBITMAP::default(),
                old: HGDIOBJ::default(),
                bits: std::ptr::null_mut(),
                size: (0, 0),
                built: None,
                pad: 0,
                shown: false,
                level: 0.0,
                t0: Instant::now(),
            }
        }
    }

    pub fn visible(&self) -> bool {
        self.shown
    }

    pub fn level(&self) -> f64 {
        self.level
    }

    /// Right below Henry (above her bed's back layer).
    pub fn order(&self, henry: isize) {
        if self.shown {
            crate::win::below(self.hwnd, henry);
        }
    }

    fn resize(&mut self, w: i32, h: i32) {
        unsafe {
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
            self.bmp = CreateDIBSection(Some(self.mem), &bi, DIB_RGB_COLORS, &mut bits, None, 0).expect("halo dib");
            self.old = SelectObject(self.mem, self.bmp.into());
            self.bits = bits as *mut u8;
            self.size = (w, h);
        }
    }

    /// One frame. `on` = she is being petted (and on screen); `src` = Henry's window right now
    /// (`Overlay::buffer`); `size` = her size setting in px (the glow scales with her).
    pub fn step(&mut self, now: Instant, dt: f64, on: bool, henry: isize, src: Option<(&[u8], (i32, i32), (i32, i32), u64)>, size: u32) {
        let target = if on && src.is_some() { 1.0 } else { 0.0 };
        // quick to come, slow to fade (like a purr winding down)
        let rate = if target > self.level { dt / 0.35 } else { dt / 0.9 };
        self.level += (target - self.level).clamp(-rate, rate);
        let Some((px, pos, (sw, sh), gen)) = src.filter(|_| self.level > 0.0) else {
            if self.shown {
                self.shown = false;
                crate::win::set_visible(self.hwnd, false);
            }
            if self.level <= 0.0 {
                self.t0 = now; // the next petting starts on the rise of a breath
            }
            return;
        };
        let pad = ((size as f64 * 0.16).round() as i32).clamp(8, 56);
        if self.built != Some((gen, (sw, sh))) || self.pad != pad {
            self.build(px, sw, sh, pad);
            self.built = Some((gen, (sw, sh)));
        }
        // breathing: 60 % .. 100 % of the glow, eased in by the level
        let t = (now - self.t0).as_secs_f64();
        let breath = 0.5 - 0.5 * (t * std::f64::consts::TAU / BREATH).cos();
        let e = self.level * self.level * (3.0 - 2.0 * self.level);
        let alpha = (255.0 * e * (0.6 + 0.4 * breath)).round().clamp(0.0, 255.0) as u8;
        let (w, h) = self.size;
        unsafe {
            let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: alpha, AlphaFormat: AC_SRC_ALPHA as u8 };
            let _ = UpdateLayeredWindow(
                HWND(self.hwnd as *mut _),
                None,
                Some(&POINT { x: pos.0 - pad, y: pos.1 - pad }),
                Some(&SIZE { cx: w, cy: h }),
                Some(self.mem),
                Some(&POINT { x: 0, y: 0 }),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
        if !self.shown {
            self.shown = true;
            crate::win::set_visible(self.hwnd, true);
            crate::win::below(self.hwnd, henry);
        }
    }

    /// Glow image: her alpha, grown by `pad` and blurred (3 box passes ≈ gaussian), in pink.
    fn build(&mut self, src: &[u8], sw: i32, sh: i32, pad: i32) {
        let (w, h) = (sw + 2 * pad, sh + 2 * pad);
        if self.size != (w, h) {
            self.resize(w, h);
        }
        self.pad = pad;
        let mut a = vec![0f32; (w * h) as usize];
        for y in 0..sh {
            for x in 0..sw {
                let v = src[((y * sw + x) * 4 + 3) as usize];
                if v > 0 {
                    a[((y + pad) * w + x + pad) as usize] = v as f32 / 255.0;
                }
            }
        }
        let r = (pad / 2).max(1);
        let mut tmp = vec![0f32; a.len()];
        for _ in 0..3 {
            box_blur(&a, &mut tmp, w, h, r, true);
            box_blur(&tmp, &mut a, w, h, r, false);
        }
        let out = unsafe { std::slice::from_raw_parts_mut(self.bits, (w * h * 4) as usize) };
        for (i, v) in a.iter().enumerate() {
            // lift the soft tail so the glow reaches the edge of the pad, keep the core under 1
            let al = (v * 3.2).min(1.0).powf(0.7) * 0.9;
            let al8 = (al * 255.0).round() as u32;
            let o = i * 4;
            out[o] = (GLOW[2] as u32 * al8 / 255) as u8;
            out[o + 1] = (GLOW[1] as u32 * al8 / 255) as u8;
            out[o + 2] = (GLOW[0] as u32 * al8 / 255) as u8;
            out[o + 3] = al8 as u8;
        }
    }
}

/// One box-blur pass of radius `r` along rows (`horiz`) or columns, edges clamped to zero.
fn box_blur(src: &[f32], dst: &mut [f32], w: i32, h: i32, r: i32, horiz: bool) {
    let (n, lines) = if horiz { (w, h) } else { (h, w) };
    let at = |line: i32, k: i32| -> usize { if horiz { (line * w + k) as usize } else { (k * w + line) as usize } };
    let norm = 1.0 / (2 * r + 1) as f32;
    for line in 0..lines {
        let mut acc = 0f32;
        for k in -r..=r {
            if (0..n).contains(&k) {
                acc += src[at(line, k)];
            }
        }
        for k in 0..n {
            dst[at(line, k)] = acc * norm;
            let (add, sub) = (k + r + 1, k - r);
            if add < n {
                acc += src[at(line, add)];
            }
            if sub >= 0 {
                acc -= src[at(line, sub)];
            }
        }
    }
}
