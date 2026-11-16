//! Status card (PLAN #54): hovering Henry's tray icon shows a pixel card in her palette (mockup B,
//! docs/mockups/tray-status): a live mini-Henry, what she is doing, what Hermes is doing, her three needs
//! as 12 blocks each and her bond with the user as 6 hearts. Drawn natively like Henry herself (one layered
//! window, a 5x7 pixel font, no webview), owned and pumped by the brain thread.
//!
//! Pin (PLAN #57): the pushpin top-right pins it (stays when the cursor leaves), dragging the card moves
//! and pins it; pinned state + position live in card.json. While the cursor is over the card it catches
//! the mouse (not click-through) and stays open, so it can be reached from the tray icon.
//!
//! Motion: fades + rises in, needs fill block by block on open (each block flashes as it fills); a rising
//! need's next block fills from the bottom and pulses with a glow; a falling need's block drains, flickers
//! and drips; a block that empties fades out; the half-full heart beats; the Hermes dot blinks while it
//! works; a new activity types itself in.
use crate::brain::Mood;
use crate::render::Sheet;
use crate::win::{Monitor, Rect};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, POINT, SIZE};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, GetDC, ReleaseDC, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HDC,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, LoadCursorW, RegisterClassW, UpdateLayeredWindow, IDC_ARROW, ULW_ALPHA, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

type C = [u8; 3];
const fn hex(v: u32) -> C {
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}
// Henry's palette (art/henry/palette.json)
const INK: C = hex(0x020123);
const BG: C = hex(0x201e2c);
const BG2: C = hex(0x2f2c3f);
const LINE: C = hex(0x4a475a);
const LINE2: C = hex(0x676678);
const DIM: C = hex(0x767485);
const MUTED: C = hex(0x8d8b9e);
const SOFT: C = hex(0xabaabc);
const LABEL: C = hex(0xbcb9ca);
const WHITE: C = hex(0xf1effd);
const PINK: C = hex(0xf6a7b9);
const PINK2: C = hex(0xf9bfce);
const ROSE: C = hex(0xde6c7a);
const MAUVE: C = hex(0xa9788a);
const PLUM: C = hex(0x6b4a5d);
const GOLD: C = hex(0xc0bc74);
const OLIVE: C = hex(0x807c4d);

/// Need colours: sleepy, bored, playful.
const NEED: [C; 3] = [MAUVE, GOLD, PINK];
const NEED_LABEL: [&str; 3] = ["SLEEPY", "BORED", "PLAYFUL"];
const BLOCKS: usize = 12;
/// Henry's cell width in the portrait (units); her art fills ~70 % of the cell
const PORTRAIT_W: i32 = 80;

// Layout in card units (1 unit = `u` screen px). Everything sits on a 2-unit grid.
const CW: i32 = 320;
const CH: i32 = 210;
/// window = card + room under it for the shadow and the notch pointing at the tray icon
const WH: i32 = CH + 8;
const PAD: i32 = 12;
const COL: i32 = 100; // right column / bars / hearts start here
const RIGHT: i32 = CW - PAD;
const ROW0: i32 = 104;
const ROW_STEP: i32 = 22;
const BOND_Y: i32 = 184;
/// pushpin, top-right (units); 6x8 glyph at 2 units per pixel
const PIN_X: i32 = RIGHT - 12;
const PIN_Y: i32 = PAD;
const PIN: [&str; 8] = ["011110", "011110", "001100", "011110", "111111", "001100", "001100", "001100"];

fn mix(a: C, b: C, t: f64) -> C {
    let t = t.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| (a[i] as f64 + (b[i] as f64 - a[i] as f64) * t).round() as u8)
}

fn ease_out(t: f64) -> f64 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// 0..1 pulse with period `p` seconds.
fn pulse(t: f64, p: f64) -> f64 {
    0.5 - 0.5 * (t * std::f64::consts::TAU / p).cos()
}

// 5x7 pixel font (35 bits per glyph, row-major).
fn glyph(ch: char) -> &'static str {
    match ch {
        'A' => "01110100011000111111100011000110001",
        'B' => "11110100011000111110100011000111110",
        'C' => "01110100011000010000100001000101110",
        'D' => "11110100011000110001100011000111110",
        'E' => "11111100001000011110100001000011111",
        'F' => "11111100001000011110100001000010000",
        'G' => "01110100011000010111100011000101111",
        'H' => "10001100011000111111100011000110001",
        'I' => "01110001000010000100001000010001110",
        'J' => "00111000100001000010000101001001100",
        'K' => "10001100101010011000101001001010001",
        'L' => "10000100001000010000100001000011111",
        'M' => "10001110111010110101100011000110001",
        'N' => "10001100011100110101100111000110001",
        'O' => "01110100011000110001100011000101110",
        'P' => "11110100011000111110100001000010000",
        'Q' => "01110100011000110001101011001001101",
        'R' => "11110100011000111110101001001010001",
        'S' => "01111100001000001110000010000111110",
        'T' => "11111001000010000100001000010000100",
        'U' => "10001100011000110001100011000101110",
        'V' => "10001100011000110001100010101000100",
        'W' => "10001100011000110101101011010101010",
        'X' => "10001100010101000100010101000110001",
        'Y' => "10001100010101000100001000010000100",
        'Z' => "11111000010001000100010001000011111",
        '0' => "01110100011001110101110011000101110",
        '1' => "00100011000010000100001000010001110",
        '2' => "01110100010000100010001000100011111",
        '3' => "11111000100010000010000011000101110",
        '4' => "00010001100101010010111110001000010",
        '5' => "11111100001111000001000011000101110",
        '6' => "00110010001000011110100011000101110",
        '7' => "11111000010001000100010000100001000",
        '8' => "01110100011000101110100011000101110",
        '9' => "01110100011000101111000010001001100",
        '!' => "00100001000010000100001000000000100",
        '?' => "01110100010000100010001000000000100",
        '.' => "00000000000000000000000000110001100",
        '-' => "00000000000000011111000000000000000",
        '\'' => "00100001000100000000000000000000000",
        _ => "00000000000000000000000000000000000",
    }
}

fn text_w(s: &str, px: i32) -> i32 {
    let n = s.chars().count() as i32;
    if n == 0 {
        0
    } else {
        (n * 6 - 1) * px
    }
}

const HEART: [&str; 6] = ["0110110", "1111111", "1111111", "0111110", "0011100", "0001000"];

/// What the card shows this frame (from the brain).
pub struct Info<'a> {
    pub sheet: &'a Sheet,
    pub anim: usize,
    pub frame: u32,
    pub mirror: bool,
    pub activity: &'static str,
    pub asleep: bool,
    pub mood: Mood,
    /// sleepiness, boredom, playfulness (0..1)
    pub needs: [f64; 3],
    pub bond: f64,
    pub bond_level: &'static str,
}

/// Where the tray says its icon is, and whether the cursor is on it (from tray events).
#[derive(Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct TrayHover {
    pub rect: Option<Rect>,
    /// Enter/Move seen and no Leave since
    pub inside: bool,
    /// a click on the icon (menu) hides the card until the cursor leaves
    pub suppressed: bool,
    /// API: show it anyway (tests, screenshots)
    pub forced: bool,
}

struct Canvas<'a> {
    px: &'a mut [u8],
    w: i32,
    h: i32,
    u: i32,
}

impl Canvas<'_> {
    /// Blend colour `c` at alpha `a` over screen px (x, y). Buffer is premultiplied BGRA.
    fn dot(&mut self, x: i32, y: i32, c: C, a: f64) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h || a <= 0.0 {
            return;
        }
        let i = ((y * self.w + x) * 4) as usize;
        let a = a.min(1.0);
        let p = &mut self.px[i..i + 4];
        let inv = 1.0 - a;
        p[0] = (c[2] as f64 * a + p[0] as f64 * inv).round() as u8;
        p[1] = (c[1] as f64 * a + p[1] as f64 * inv).round() as u8;
        p[2] = (c[0] as f64 * a + p[2] as f64 * inv).round() as u8;
        p[3] = (255.0 * a + p[3] as f64 * inv).round() as u8;
    }

    /// Fill a rect given in card units.
    fn fill(&mut self, x: i32, y: i32, w: i32, h: i32, c: C, a: f64) {
        let u = self.u;
        for sy in y * u..(y + h) * u {
            for sx in x * u..(x + w) * u {
                self.dot(sx, sy, c, a);
            }
        }
    }

    /// Pixel text, `px` units per font pixel; only the first `n` chars (typing in).
    fn text(&mut self, x: i32, y: i32, s: &str, px: i32, c: C, a: f64, n: usize) {
        for (i, ch) in s.chars().take(n).enumerate() {
            let g = glyph(ch.to_ascii_uppercase());
            for (k, b) in g.bytes().enumerate() {
                if b == b'1' {
                    let (gx, gy) = ((k % 5) as i32, (k / 5) as i32);
                    self.fill(x + (i as i32 * 6 + gx) * px, y + gy * px, px, px, c, a);
                }
            }
        }
    }

    /// A box with 2-unit stepped corners: outline `o` (2 units) around fill `f`; `corner` = what shows
    /// in the cut corners (None: transparent).
    fn pixel_box(&mut self, x: i32, y: i32, w: i32, h: i32, o: C, f: C, a: f64) {
        // outline: everything but the four 2x2 corners
        self.fill(x + 2, y, w - 4, h, o, a);
        self.fill(x, y + 2, 2, h - 4, o, a);
        self.fill(x + w - 2, y + 2, 2, h - 4, o, a);
        // inside, with its own corners stepped in (the outline pixel sits on the diagonal)
        self.fill(x + 4, y + 2, w - 8, h - 4, f, a);
        self.fill(x + 2, y + 4, 2, h - 8, f, a);
        self.fill(x + w - 4, y + 4, 2, h - 8, f, a);
    }
}

pub struct Card {
    hwnd: isize,
    mem: HDC,
    bits: *mut u8,
    size: (i32, i32),
    u: i32,
    shown: bool,
    /// 0 hidden .. 1 fully in
    vis: f64,
    hover_since: Option<Instant>,
    away_since: Option<Instant>,
    opened: Instant,
    pos: (i32, i32),
    notch_x: i32,
    notch_down: bool,
    /// displayed needs (they catch up with the real ones: fill-in on open, smooth drains)
    disp: [f64; 3],
    full: [usize; 3],
    pop: [[Option<Instant>; BLOCKS]; 3],
    ghost: [[Option<Instant>; BLOCKS]; 3],
    /// trend per need: +1 rising, -1 falling, 0 flat (from the real values, sampled)
    trend: [i8; 3],
    sample: ([f64; 3], Instant),
    activity: &'static str,
    activity_since: Instant,
    /// portrait cells for (skin, row, frame)
    cells: HashMap<(&'static str, u32, u32), (i32, i32, Vec<u8>)>,
    /// art centre x (cell px) per (skin, animation)
    mids: HashMap<(&'static str, usize), f64>,
    t0: Instant,
    last_top: Instant,
    /// card.json: pinned + position
    store: PathBuf,
    pinned: bool,
    /// a position worth keeping (placed once, loaded or dragged)
    has_pos: bool,
    /// dragging: grab offset from the card's corner, where the drag started
    drag: Option<(i32, i32, (i32, i32))>,
    prev_l: bool,
    interactive: bool,
    over_pin: bool,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Stored {
    pinned: bool,
    x: i32,
    y: i32,
}

impl Card {
    pub fn new(store: PathBuf) -> Card {
        let saved: Option<Stored> = std::fs::read_to_string(&store).ok().and_then(|t| serde_json::from_str(&t).ok());
        unsafe {
            let inst = GetModuleHandleW(None).expect("module handle");
            let class = w!("HenryPetCard");
            let cursor = LoadCursorW(None, IDC_ARROW).unwrap_or_default();
            let wc = WNDCLASSW { lpfnWndProc: Some(crate::render::wndproc), hInstance: inst.into(), lpszClassName: class, hCursor: cursor, ..Default::default() };
            RegisterClassW(&wc);
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class,
                w!("Henry status"),
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
            .expect("card window");
            let screen = GetDC(None);
            let mem = CreateCompatibleDC(Some(screen));
            ReleaseDC(None, screen);
            // 1 unit = 1 px up to 125 %, 2 px from 150 %
            let u = (GetDpiForSystem() as f64 / 96.0).round().max(1.0) as i32;
            let now = Instant::now();
            Card {
                hwnd: hwnd.0 as isize,
                mem,
                bits: std::ptr::null_mut(),
                size: (0, 0),
                u,
                shown: false,
                vis: 0.0,
                hover_since: None,
                away_since: None,
                opened: now,
                pos: saved.as_ref().map_or((0, 0), |s| (s.x, s.y)),
                notch_x: CW / 2,
                notch_down: true,
                disp: [0.0; 3],
                full: [0; 3],
                pop: [[None; BLOCKS]; 3],
                ghost: [[None; BLOCKS]; 3],
                trend: [0; 3],
                sample: ([0.0; 3], now),
                activity: "",
                activity_since: now,
                cells: HashMap::new(),
                mids: HashMap::new(),
                t0: now,
                last_top: now,
                store,
                pinned: saved.as_ref().is_some_and(|s| s.pinned),
                has_pos: saved.is_some(),
                drag: None,
                prev_l: false,
                interactive: false,
                over_pin: false,
            }
        }
    }

    pub fn visible(&self) -> bool {
        self.shown
    }

    pub fn pinned(&self) -> bool {
        self.pinned
    }

    fn save(&self) {
        let s = Stored { pinned: self.pinned, x: self.pos.0, y: self.pos.1 };
        if let Ok(t) = serde_json::to_string_pretty(&s) {
            let _ = std::fs::write(&self.store, t);
        }
    }

    /// API: pin / unpin, and/or move it (moving pins it).
    pub fn set_pin(&mut self, on: Option<bool>, pos: Option<(i32, i32)>, mons: &[Monitor]) {
        if let Some(p) = pos {
            let c = (p.0 + CW * self.u / 2, p.1 + CH * self.u / 2);
            self.pos = self.clamp_pos(p, mons, c);
            self.has_pos = true;
            self.pinned = true;
        }
        if let Some(on) = on {
            self.pinned = on;
        }
        self.save();
    }

    /// Keep the card on the work area of the monitor under `anchor` (cursor / its centre).
    fn clamp_pos(&self, p: (i32, i32), mons: &[Monitor], anchor: (i32, i32)) -> (i32, i32) {
        let u = self.u;
        let m = mons
            .iter()
            .find(|m| m.rect.left <= anchor.0 && anchor.0 < m.rect.right && m.rect.top <= anchor.1 && anchor.1 < m.rect.bottom)
            .or_else(|| mons.iter().find(|m| m.primary))
            .or(mons.first());
        let Some(m) = m else { return p };
        let wk = m.work;
        (p.0.clamp(wk.left, (wk.right - CW * u).max(wk.left)), p.1.clamp(wk.top, (wk.bottom - WH * u).max(wk.top)))
    }

    /// Back on top (call right after Henry / her bed re-assert their own topmost).
    pub fn raise(&mut self) {
        if self.shown {
            crate::win::topmost(self.hwnd);
        }
    }

    /// Window rect on screen (x, y, w, h) when it was last placed.
    pub fn rect(&self) -> (i32, i32, i32, i32) {
        (self.pos.0, self.pos.1, CW * self.u, WH * self.u)
    }

    fn ensure_buffer(&mut self, w: i32, h: i32) {
        if self.size == (w, h) {
            return;
        }
        unsafe {
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
            let bmp = CreateDIBSection(Some(self.mem), &bi, DIB_RGB_COLORS, &mut bits, None, 0).expect("card dib");
            // one size for the whole run (units are fixed at start), so this runs once
            SelectObject(self.mem, bmp.into());
            self.bits = bits as *mut u8;
            self.size = (w, h);
        }
    }

    /// Place the card over the icon: above the taskbar when it is at the bottom, else below the icon.
    fn place(&mut self, icon: Rect, mons: &[Monitor]) {
        let u = self.u;
        let (icx, icy) = ((icon.left + icon.right) / 2, (icon.top + icon.bottom) / 2);
        let m = mons.iter().find(|m| m.rect.left <= icx && icx < m.rect.right && m.rect.top <= icy && icy < m.rect.bottom).or(mons.first());
        let (work, full) = m.map_or((icon, icon), |m| (m.work, m.rect));
        let (w, h) = (CW * u, WH * u);
        let margin = 8 * u;
        let x = (icx - w / 2).clamp(work.left + margin, (work.right - margin - w).max(work.left + margin));
        // taskbar at the bottom (icon below the work area): sit on it, notch down at the icon
        let below_work = icon.top >= work.bottom - 2;
        let y = if below_work || icon.top - h - 4 * u >= full.top {
            self.notch_down = true;
            icon.top.min(work.bottom) - 4 * u - (CH + 6) * u
        } else {
            self.notch_down = false;
            icon.bottom + 4 * u
        };
        self.pos = (x, y);
        // notch centred on the icon, kept on the card's straight edge (even units: 2-unit grid)
        self.notch_x = (((icx - x) / u) / 2 * 2).clamp(14, CW - 14);
    }

    /// One frame: decide visibility from the tray hover, animate, draw.
    /// `lbutton` = left mouse button down now; `hide_pinned` = Henry is hidden / fullscreen app on its
    /// screen (a pinned card steps aside too).
    #[allow(clippy::too_many_arguments)]
    pub fn step(&mut self, now: Instant, dt: f64, cursor: (i32, i32), lbutton: bool, hide_pinned: bool, tray: TrayHover, mons: &[Monitor], info: &Info) {
        // --- trends: sample the real needs every 0.5 s ---
        let since = (now - self.sample.1).as_secs_f64();
        if since >= 0.5 {
            for i in 0..3 {
                let d = info.needs[i] - self.sample.0[i];
                self.trend[i] = if d > 1e-7 { 1 } else if d < -1e-7 { -1 } else { 0 };
            }
            self.sample = (info.needs, now);
        }

        // --- mouse on the card: pin button, drag to move (moving pins it) ---
        let u = self.u;
        let over = self.shown
            && cursor.0 >= self.pos.0
            && cursor.0 < self.pos.0 + CW * u
            && cursor.1 >= self.pos.1
            && cursor.1 < self.pos.1 + WH * u;
        let (lx, ly) = ((cursor.0 - self.pos.0) / u, (cursor.1 - self.pos.1) / u);
        self.over_pin = over && lx >= PIN_X - 6 && lx < PIN_X + 18 && ly >= PIN_Y - 6 && ly < PIN_Y + 22;
        let press = lbutton && !self.prev_l;
        self.prev_l = lbutton;
        if press && over && self.vis > 0.5 {
            if self.over_pin {
                self.pinned = !self.pinned;
                self.has_pos = true;
                self.save();
            } else {
                self.drag = Some((cursor.0 - self.pos.0, cursor.1 - self.pos.1, self.pos));
            }
        }
        if let Some((ox, oy, start)) = self.drag {
            if lbutton {
                let np = self.clamp_pos((cursor.0 - ox, cursor.1 - oy), mons, cursor);
                if np != self.pos {
                    if !self.pinned && (np.0 - start.0).abs() + (np.1 - start.1).abs() > 3 {
                        self.pinned = true;
                    }
                    self.pos = np;
                    self.has_pos = true;
                }
            } else {
                self.drag = None;
                if self.pinned {
                    self.save();
                }
            }
        }
        let inter = self.shown && (over || self.drag.is_some());
        if inter != self.interactive {
            self.interactive = inter;
            crate::win::set_click_through(self.hwnd, !inter);
        }

        // --- hover ---
        let over_icon = tray.rect.is_some_and(|r| {
            let pad = 2;
            tray.inside && cursor.0 >= r.left - pad && cursor.0 < r.right + pad && cursor.1 >= r.top - pad && cursor.1 < r.bottom + pad
        });
        let pinned_on = self.pinned && !hide_pinned;
        let want = tray.forced || pinned_on || (over_icon && !tray.suppressed) || over || self.drag.is_some();
        if want {
            self.away_since = None;
            let t = *self.hover_since.get_or_insert(now);
            let delay = if tray.forced || pinned_on || over { 0.0 } else { 0.28 };
            let target_on = (now - t).as_secs_f64() >= delay;
            if target_on && self.vis == 0.0 {
                // opening: a pinned card where it was left (if that is still on a screen), else at the icon;
                // fill the bars in from empty
                let centre = (self.pos.0 + CW * u / 2, self.pos.1 + CH * u / 2);
                let on_screen = mons.iter().any(|m| m.rect.left <= centre.0 && centre.0 < m.rect.right && m.rect.top <= centre.1 && centre.1 < m.rect.bottom);
                if self.pinned && self.has_pos && on_screen {
                    self.pos = self.clamp_pos(self.pos, mons, centre);
                } else {
                    let icon = tray.rect.unwrap_or_else(|| {
                        let p = mons.iter().find(|m| m.primary).or(mons.first());
                        p.map_or(Rect { left: 0, top: 0, right: 1, bottom: 1 }, |m| Rect { left: m.rect.right - 140, top: m.work.bottom + 4, right: m.rect.right - 116, bottom: m.rect.bottom - 4 })
                    });
                    self.place(icon, mons);
                    self.has_pos = true;
                }
                self.opened = now;
                self.disp = [0.0; 3];
                self.full = [0; 3];
                self.pop = [[None; BLOCKS]; 3];
                self.ghost = [[None; BLOCKS]; 3];
                self.activity_since = now;
                self.activity = info.activity;
            }
            if target_on {
                self.vis = (self.vis + dt / 0.16).min(1.0);
            }
        } else {
            self.hover_since = None;
            let t = *self.away_since.get_or_insert(now);
            if (now - t).as_secs_f64() > 0.08 {
                self.vis = (self.vis - dt / 0.11).max(0.0);
            }
        }
        if self.vis <= 0.0 {
            if self.shown {
                self.shown = false;
                crate::win::set_visible(self.hwnd, false);
            }
            return;
        }

        // --- animate values ---
        for i in 0..3 {
            let real = info.needs[i].clamp(0.0, 1.0);
            let open_t = (now - self.opened).as_secs_f64();
            // fill-in on open waits for the card to be mostly in, staggered per row
            if open_t < 0.12 + i as f64 * 0.05 {
                continue;
            }
            let step = dt * 2.4;
            self.disp[i] += (real - self.disp[i]).clamp(-step, step);
            let full = ((self.disp[i] * BLOCKS as f64) + 1e-9).floor() as usize;
            let full = full.min(BLOCKS);
            if full > self.full[i] {
                for b in self.full[i]..full {
                    self.pop[i][b] = Some(now);
                    self.ghost[i][b] = None;
                }
            } else if full < self.full[i] {
                for b in full..self.full[i] {
                    self.ghost[i][b] = Some(now);
                    self.pop[i][b] = None;
                }
            }
            self.full[i] = full;
        }
        if info.activity != self.activity {
            self.activity = info.activity;
            self.activity_since = now;
        }

        // --- draw ---
        let (w, h) = (CW * u, WH * u);
        self.ensure_buffer(w, h);
        let buf = unsafe { std::slice::from_raw_parts_mut(self.bits, (w * h * 4) as usize) };
        buf.fill(0);
        let t = (now - self.t0).as_secs_f64();
        let since_open = (now - self.opened).as_secs_f64();
        {
            let mut cv = Canvas { px: buf, w, h, u };
            // shadow (4 units down, soft ink) + card
            cv.pixel_box(0, 4, CW, CH, INK, INK, 0.38);
            cv.pixel_box(0, 0, CW, CH, LINE, BG, 1.0);
            self.draw_notch(&mut cv);
            self.draw_portrait(&mut cv, info, t);
            self.draw_header(&mut cv, info, t, now);
            self.draw_pin(&mut cv);
            for i in 0..3 {
                self.draw_need(&mut cv, i, ROW0 + i as i32 * ROW_STEP, t, now);
            }
            cv.fill(PAD, 172, CW - 2 * PAD, 2, BG2, 1.0);
            self.draw_bond(&mut cv, info, t, since_open);
        }

        // --- show: fade + rise into place ---
        let e = ease_out(self.vis);
        let rise = ((1.0 - e) * 8.0 * u as f64).round() as i32 * if self.notch_down { 1 } else { -1 };
        unsafe {
            let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: (255.0 * e).round() as u8, AlphaFormat: AC_SRC_ALPHA as u8 };
            let _ = UpdateLayeredWindow(
                HWND(self.hwnd as *mut _),
                None,
                Some(&POINT { x: self.pos.0, y: self.pos.1 + rise }),
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
        }
        // above Henry and her bed (they re-assert topmost every 1.5 s themselves)
        if (now - self.last_top).as_secs_f64() > 0.25 {
            self.last_top = now;
            crate::win::topmost(self.hwnd);
        }
    }

    /// Stepped triangle from the card edge to the icon (bottom edge when the card sits above the icon).
    /// Pushpin: dim when free, lighter under the cursor, pink and pushed in (needle hidden) when pinned.
    fn draw_pin(&self, cv: &mut Canvas) {
        let (head, needle, rows, dy) = if self.pinned {
            (if self.over_pin { PINK2 } else { PINK }, MUTED, 6, 2)
        } else if self.over_pin {
            (SOFT, MUTED, 8, 0)
        } else {
            (LINE2, LINE, 8, 0)
        };
        for (ry, row) in PIN.iter().take(rows).enumerate() {
            for (rx, b) in row.bytes().enumerate() {
                if b == b'1' {
                    let c = if ry >= 5 { needle } else { head };
                    cv.fill(PIN_X + rx as i32 * 2, PIN_Y + dy + ry as i32 * 2, 2, 2, c, 1.0);
                }
            }
        }
        if self.pinned {
            // shine on the head
            cv.fill(PIN_X + 2, PIN_Y + dy, 2, 2, WHITE, 0.7);
        }
    }

    fn draw_notch(&self, cv: &mut Canvas) {
        let nx = self.notch_x;
        // a pinned / moved card points at nothing
        if self.pinned || self.drag.is_some() {
            return;
        }
        if self.notch_down {
            // open the bottom border, then three 2-unit rows narrowing to the tip
            cv.fill(nx - 4, CH - 2, 8, 2, BG, 1.0);
            cv.fill(nx - 6, CH, 12, 2, LINE, 1.0);
            cv.fill(nx - 4, CH, 8, 2, BG, 1.0);
            cv.fill(nx - 4, CH + 2, 8, 2, LINE, 1.0);
            cv.fill(nx - 2, CH + 2, 4, 2, BG, 1.0);
            cv.fill(nx - 2, CH + 4, 4, 2, LINE, 1.0);
        } else {
            // card below the icon: no notch (the shadow side is down)
        }
    }

    fn draw_portrait(&mut self, cv: &mut Canvas, info: &Info, t: f64) {
        let (bx, by, bw, bh) = (PAD, PAD, 76, 76);
        cv.pixel_box(bx, by, bw, bh, BG2, BG2, 1.0);
        // floor strip with a lighter lip
        let floor = by + bh - 8;
        cv.fill(bx + 2, floor, bw - 4, 6, LINE, 1.0);
        cv.fill(bx + 2, floor, bw - 4, 2, LINE2, 1.0);
        cv.fill(bx + 4, by + bh - 2, bw - 8, 2, LINE, 1.0);

        // live mini-Henry: her current frame at 88 units per cell width (her body fills the box), feet on the floor strip
        let sheet = info.sheet;
        let a = &sheet.anims[info.anim.min(sheet.anims.len() - 1)];
        let u = cv.u;
        let sw = PORTRAIT_W * u;
        let sh = (sheet.cell_h as f64 * sw as f64 / sheet.cell_w as f64).round() as i32;
        let key = (sheet.skin, a.row, info.frame);
        if !self.cells.contains_key(&key) {
            if self.cells.len() > 160 {
                self.cells.clear();
            }
            let px = sheet.scaled_cell(a.row, info.frame, sw as u32, sh as u32);
            self.cells.insert(key, (sw, sh, px));
        }
        let (cw, ch, px) = &self.cells[&key];
        let foot = (sheet.info.foot_y * *ch as f64 / sheet.cell_h as f64).round() as i32;
        // centred on the animation's art (all its frames, so she doesn't jitter): walk art sits off-centre
        let anim_i = info.anim.min(sheet.anims.len() - 1);
        let mid = *self.mids.entry((sheet.skin, anim_i)).or_insert_with(|| {
            let (mut lo, mut hi) = (i32::MAX, i32::MIN);
            for &f in &a.seq {
                for x in 0..sheet.cell_w as i32 {
                    if (0..sheet.cell_h as i32).step_by(2).any(|y| sheet.opaque_at(anim_i, f, x, y)) {
                        lo = lo.min(x);
                        hi = hi.max(x);
                    }
                }
            }
            if lo > hi { sheet.cell_w as f64 / 2.0 } else { (lo + hi + 1) as f64 / 2.0 }
        });
        let mid = if info.mirror { sheet.cell_w as f64 - mid } else { mid };
        let ox = ((bx + bw / 2) * u) as f64 - mid * *cw as f64 / sheet.cell_w as f64;
        let ox = ox.round() as i32;
        let oy = (floor + 1) * u - foot; // paws sink one unit into the lip
        let (clip_l, clip_t, clip_r, clip_b) = ((bx + 2) * u, (by + 2) * u, (bx + bw - 2) * u, (floor + 2) * u);
        for yy in 0..*ch {
            let sy = oy + yy;
            if sy < clip_t || sy >= clip_b {
                continue;
            }
            for xx in 0..*cw {
                let sx = ox + xx;
                if sx < clip_l || sx >= clip_r {
                    continue;
                }
                let src_x = if info.mirror { cw - 1 - xx } else { xx };
                let i = ((yy * cw + src_x) * 4) as usize;
                let al = px[i + 3];
                if al == 0 {
                    continue;
                }
                // premultiplied source over the box
                let o = ((sy * cv.w + sx) * 4) as usize;
                let inv = 255 - al as u32;
                for c in 0..3 {
                    cv.px[o + c] = (px[i + c] as u32 + cv.px[o + c] as u32 * inv / 255).min(255) as u8;
                }
                cv.px[o + 3] = (al as u32 + cv.px[o + 3] as u32 * inv / 255).min(255) as u8;
            }
        }
        // sleeping: little z's drift up out of the portrait
        if info.asleep {
            for k in 0..2 {
                let p = ((t / 2.4) + k as f64 * 0.5).fract();
                let zx = bx + 50 + (p * 8.0) as i32 / 2 * 2;
                let zy = by + 30 - (p * 22.0) as i32 / 2 * 2;
                let a = if p < 0.2 { p / 0.2 } else { 1.0 - (p - 0.2) / 0.8 };
                cv.text(zx, zy, "Z", 1 + k as i32 % 2, SOFT, a * 0.9, 1);
            }
        }
    }

    fn draw_header(&self, cv: &mut Canvas, info: &Info, t: f64, now: Instant) {
        cv.text(COL, 12, "HENRY", 3, WHITE, 1.0, 5);
        // what she does: types itself in when it changes
        let typed = ((now - self.activity_since).as_secs_f64() / 0.022) as usize + 1;
        cv.text(COL, 42, self.activity, 2, SOFT, 1.0, typed);
        let n = self.activity.chars().count();
        if typed <= n {
            let cx = COL + typed.min(n) as i32 * 12;
            cv.fill(cx, 42, 2, 14, SOFT, 0.8);
        }
        // Hermes tag
        let (label, bg, fg, dot, blink) = match info.mood {
            Mood::Run => ("HERMES WORKING", PLUM, PINK2, PINK, Some(0.9)),
            Mood::Review => ("HERMES THINKING", PLUM, PINK2, PINK, Some(1.8)),
            Mood::Waiting => ("HERMES ASKS YOU", OLIVE, WHITE, WHITE, Some(0.5)),
            Mood::Failed => ("HERMES FAILED", ROSE, WHITE, WHITE, None),
            _ => ("HERMES IDLE", BG2, MUTED, LINE2, None),
        };
        let tw = text_w(label, 2);
        let (x, y, wdt, ht) = (COL, 64, 6 + 6 + 6 + tw + 6, 24);
        cv.pixel_box(x, y, wdt, ht, bg, bg, 1.0);
        let da = blink.map_or(1.0, |p| 0.25 + 0.75 * pulse(t, p));
        cv.fill(x + 6, y + 9, 6, 6, bg, 1.0);
        cv.fill(x + 6, y + 9, 6, 6, dot, da);
        cv.text(x + 18, y + 5, label, 2, fg, 1.0, usize::MAX);
    }

    fn draw_need(&self, cv: &mut Canvas, i: usize, y: i32, t: f64, now: Instant) {
        cv.text(PAD, y, NEED_LABEL[i], 2, LABEL, 1.0, usize::MAX);
        let c = NEED[i];
        let (bw, bh, gap) = (14, 10, 2);
        let by = y + 2;
        let v = self.disp[i] * BLOCKS as f64;
        let full = self.full[i];
        let frac = (v - full as f64).clamp(0.0, 1.0);
        let trend = self.trend[i];
        for b in 0..BLOCKS {
            let bx = COL + b as i32 * (bw + gap);
            let mut col = BG2;
            if b < full {
                col = c;
                if let Some(p) = self.pop[i][b] {
                    let k = (now - p).as_secs_f64() / 0.22;
                    if k < 1.0 {
                        col = mix(WHITE, c, ease_out(k));
                    }
                }
            } else if let Some(g) = self.ghost[i][b] {
                let k = (now - g).as_secs_f64() / 0.3;
                if k < 1.0 {
                    col = mix(mix(c, WHITE, 0.25), BG2, ease_out(k));
                }
            }
            cv.fill(bx, by, bw, bh, col, 1.0);
            // the block in play: partly full
            if b == full && full < BLOCKS {
                let lvl = ((frac * bh as f64) / 2.0).round() as i32 * 2; // 2-unit steps
                match trend {
                    1 => {
                        // rising: glows and pulses, fills from the bottom
                        let p = pulse(t, 1.3);
                        cv.fill(bx, by, bw, bh, mix(BG2, c, 0.12 + 0.22 * p), 1.0);
                        if lvl > 0 {
                            cv.fill(bx, by + bh - lvl, bw, lvl, mix(BG2, c, 0.55 + 0.25 * p), 1.0);
                        }
                        // glow ring in the gap around it
                        let ga = 0.15 + 0.55 * p;
                        cv.fill(bx - 1, by - 1, bw + 2, 1, c, ga);
                        cv.fill(bx - 1, by + bh, bw + 2, 1, c, ga);
                        cv.fill(bx - 1, by, 1, bh, c, ga);
                        cv.fill(bx + bw, by, 1, bh, c, ga);
                    }
                    -1 => {
                        // falling: what is left fades in and out, its surface sinks, drops fall out of it
                        let p = pulse(t, 0.9);
                        if lvl > 0 {
                            cv.fill(bx, by + bh - lvl, bw, lvl, mix(c, BG2, 0.1 + 0.5 * p), 1.0);
                            if lvl >= 4 {
                                cv.fill(bx, by + bh - lvl, bw, 2, mix(c, WHITE, 0.2), 0.6);
                            }
                        }
                        let d = (t / 1.1).fract();
                        let dy = by + bh + ((d * d) * 10.0) as i32 / 2 * 2; // accelerates as it falls
                        cv.fill(bx + bw / 2 - 1, dy, 2, 2, c, (1.0 - d) * 0.9);
                    }
                    _ => {
                        if lvl > 0 {
                            cv.fill(bx, by + bh - lvl, bw, lvl, mix(BG2, c, 0.6), 1.0);
                        }
                    }
                }
            }
            // a falling need's top full block dims gently too, so the direction reads even at a block edge
            if trend == -1 && b + 1 == full && frac < 0.05 {
                cv.fill(bx, by, bw, bh, BG2, 0.25 * pulse(t, 0.9));
            }
        }
        // trend arrow in the right column
        let ax = RIGHT - 10;
        let ay = by + 2;
        match trend {
            1 => {
                cv.fill(ax + 4, ay, 2, 2, c, 1.0);
                cv.fill(ax + 2, ay + 2, 6, 2, c, 1.0);
                cv.fill(ax, ay + 4, 10, 2, c, 1.0);
            }
            -1 => {
                let dc = mix(c, BG2, 0.45);
                cv.fill(ax, ay, 10, 2, dc, 1.0);
                cv.fill(ax + 2, ay + 2, 6, 2, dc, 1.0);
                cv.fill(ax + 4, ay + 4, 2, 2, dc, 1.0);
            }
            _ => cv.fill(ax + 2, ay + 2, 6, 2, LINE, 1.0),
        }
    }

    fn draw_bond(&self, cv: &mut Canvas, info: &Info, t: f64, since_open: f64) {
        let y = BOND_Y;
        cv.text(PAD, y, "BOND", 2, LABEL, 1.0, usize::MAX);
        let hearts = info.bond.clamp(0.0, 1.0) * 6.0;
        for k in 0..6 {
            let hx = COL + k * 18;
            let hy = y + 1;
            // hearts appear one by one on open
            let appear = ((since_open - 0.15 - k as f64 * 0.04) / 0.12).clamp(0.0, 1.0);
            if appear <= 0.0 {
                continue;
            }
            let fill = (hearts - k as f64).clamp(0.0, 1.0);
            let cols = (fill * 7.0).round() as i32;
            let partial = fill > 0.0 && fill < 1.0;
            // the half-full heart beats: two quick brightenings every 1.6 s
            let ph = (t % 1.6) / 1.6;
            let beat = if partial && (ph < 0.07 || (0.16..0.23).contains(&ph)) { 1.0 } else { 0.0 };
            for (ry, row) in HEART.iter().enumerate() {
                for (rx, b) in row.bytes().enumerate() {
                    if b != b'1' {
                        continue;
                    }
                    let on = (rx as i32) < cols;
                    let mut c = if on { mix(ROSE, PINK, beat) } else { BG2 };
                    if on && rx == 1 && ry == 1 {
                        c = PINK2; // shine
                    }
                    cv.fill(hx + rx as i32 * 2, hy + ry as i32 * 2, 2, 2, c, appear);
                }
            }
        }
        let lvl = info.bond_level.to_ascii_uppercase();
        let lc = match info.bond_level {
            "awful" => DIM,
            "bad" => MAUVE,
            "neutral" => PINK,
            _ => PINK2,
        };
        cv.text(RIGHT - text_w(&lvl, 2), y, &lvl, 2, lc, 1.0, usize::MAX);
    }
}
