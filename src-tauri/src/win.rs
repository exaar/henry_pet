//! Thin Win32 layer: monitors, foreground-window checks, moving the overlay. All coordinates are physical pixels.
use windows::core::BOOL;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, MonitorFromWindow, HDC, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
use windows::Win32::System::SystemInformation::{GetLocalTime, GetTickCount};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetCursorPos, GetForegroundWindow, InternalGetWindowText, GetGUIThreadInfo, GUITHREADINFO, GetSystemMetrics, SM_SWAPBUTTON, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible, IsZoomed, SetWindowLongPtrW, SetWindowPos, ShowWindowAsync, GWL_EXSTYLE, HWND_TOPMOST,
    SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE,
    WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
};

#[derive(Clone, Copy, Debug, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl From<RECT> for Rect {
    fn from(r: RECT) -> Self {
        Rect { left: r.left, top: r.top, right: r.right, bottom: r.bottom }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Monitor {
    pub handle: isize,
    pub rect: Rect,
    pub work: Rect,
    pub primary: bool,
}

fn h(hwnd: isize) -> HWND {
    HWND(hwnd as *mut core::ffi::c_void)
}

fn monitor_info(m: HMONITOR) -> Option<MONITORINFO> {
    let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe { GetMonitorInfoW(m, &mut mi).as_bool() }.then_some(mi)
}

pub fn monitors() -> Vec<Monitor> {
    unsafe extern "system" fn cb(m: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        let out = &mut *(data.0 as *mut Vec<Monitor>);
        if let Some(mi) = monitor_info(m) {
            out.push(Monitor {
                handle: m.0 as isize,
                rect: mi.rcMonitor.into(),
                work: mi.rcWork.into(),
                primary: mi.dwFlags & 1 != 0,
            });
        }
        BOOL(1)
    }
    let mut out: Vec<Monitor> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(cb), LPARAM(&mut out as *mut _ as isize));
    }
    out
}

pub struct Foreground {
    /// Monitor fully covered by a fullscreen (not merely maximized) foreground window.
    pub fullscreen_monitor: Option<isize>,
    pub exe: Option<String>,
}

const SHELL_CLASSES: [&str; 4] = ["Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd"];

pub fn foreground(own: isize) -> Foreground {
    let none = Foreground { fullscreen_monitor: None, exe: None };
    unsafe {
        let fg = GetForegroundWindow();
        if fg.0.is_null() || fg.0 as isize == own {
            return none;
        }
        let exe = process_exe(fg);
        let mut buf = [0u16; 128];
        let n = GetClassNameW(fg, &mut buf) as usize;
        let class = String::from_utf16_lossy(&buf[..n]);
        if SHELL_CLASSES.contains(&class.as_str()) || !IsWindowVisible(fg).as_bool() || IsIconic(fg).as_bool() || IsZoomed(fg).as_bool() {
            return Foreground { fullscreen_monitor: None, exe };
        }
        let mut r = RECT::default();
        if GetWindowRect(fg, &mut r).is_err() {
            return Foreground { fullscreen_monitor: None, exe };
        }
        let mon = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
        let full = monitor_info(mon).map(|mi| {
            let m = mi.rcMonitor;
            r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom
        });
        Foreground { fullscreen_monitor: (full == Some(true)).then_some(mon.0 as isize), exe }
    }
}

unsafe fn process_exe(hwnd: HWND) -> Option<String> {
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    pid_exe(pid)
}

/// Executable file name of a process ("Resolve.exe").
pub fn pid_exe(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    unsafe { pid_exe_inner(pid) }
}

unsafe fn pid_exe_inner(pid: u32) -> Option<String> {
    let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let ok = QueryFullProcessImageNameW(proc, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
    let _ = CloseHandle(proc);
    if !ok {
        return None;
    }
    let path = String::from_utf16_lossy(&buf[..len as usize]);
    path.rsplit('\\').next().map(str::to_string)
}

/// Toggle click-through by flipping WS_EX_TRANSPARENT only. Tauri's set_ignore_cursor_events rewrites the
/// whole extended style from its own state and drops WS_EX_TOOLWINDOW/NOACTIVATE (taskbar button appears).
/// Top-level ancestor of a window.
pub fn root(hwnd: isize) -> isize {
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetAncestor(h(hwnd), windows::Win32::UI::WindowsAndMessaging::GA_ROOT).0 as isize }
}

/// Replay (tape.rs): her windows are never shown, raised or made clickable.
pub static HEADLESS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn headless() -> bool {
    HEADLESS.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_click_through(hwnd: isize, on: bool) {
    if headless() {
        return;
    }
    unsafe {
        let ex = GetWindowLongPtrW(h(hwnd), GWL_EXSTYLE);
        let t = windows::Win32::UI::WindowsAndMessaging::WS_EX_TRANSPARENT.0 as isize;
        let ex = if on { ex | t } else { ex & !t };
        SetWindowLongPtrW(h(hwnd), GWL_EXSTYLE, ex);
    }
}

pub fn topmost(hwnd: isize) {
    if headless() {
        return;
    }
    unsafe {
        let _ = SetWindowPos(
            h(hwnd),
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
        );
    }
}

/// Put `hwnd` right below `above` in the z-order (both topmost: stays in the topmost band).
pub fn below(hwnd: isize, above: isize) {
    if headless() {
        return;
    }
    unsafe {
        let _ = SetWindowPos(h(hwnd), Some(h(above)), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

pub fn set_visible(hwnd: isize, visible: bool) {
    if headless() {
        return;
    }
    unsafe {
        let _ = ShowWindowAsync(h(hwnd), if visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
    }
}

/// Time since the last keyboard/mouse input anywhere in the session.
pub fn user_idle() -> std::time::Duration {
    unsafe {
        let mut li = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
        if !GetLastInputInfo(&mut li).as_bool() {
            return std::time::Duration::ZERO;
        }
        std::time::Duration::from_millis(GetTickCount().wrapping_sub(li.dwTime) as u64)
    }
}

pub fn local_hour() -> u32 {
    unsafe { GetLocalTime().wHour as u32 }
}

pub fn cursor() -> (i32, i32) {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x, p.y)
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct WinInfo {
    pub hwnd: isize,
    /// Visible frame (DWM extended frame bounds, without the invisible resize borders).
    pub rect: Rect,
    pub maximized: bool,
    pub pid: u32,
}

/// Visible top-level app windows in z-order, topmost first (taskbar/desktop/tool windows skipped).
pub fn windows(own: isize) -> Vec<WinInfo> {
    unsafe extern "system" fn cb(hw: HWND, data: LPARAM) -> BOOL {
        let (own, out) = &mut *(data.0 as *mut (isize, Vec<WinInfo>));
        if hw.0 as isize == *own || !IsWindowVisible(hw).as_bool() || IsIconic(hw).as_bool() {
            return BOOL(1);
        }
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(hw, DWMWA_CLOAKED, &mut cloaked as *mut _ as *mut _, 4);
        if cloaked != 0 {
            return BOOL(1);
        }
        let ex = GetWindowLongPtrW(hw, GWL_EXSTYLE) as u32;
        if ex & WS_EX_TOOLWINDOW.0 != 0 && ex & WS_EX_APPWINDOW.0 == 0 || ex & WS_EX_NOACTIVATE.0 != 0 {
            return BOOL(1);
        }
        // InternalGetWindowText never sends a message: GetWindowTextLength blocks on a hung app's window.
        let mut title = [0u16; 8];
        if InternalGetWindowText(hw, &mut title) == 0 {
            return BOOL(1);
        }
        let mut buf = [0u16; 64];
        let n = GetClassNameW(hw, &mut buf) as usize;
        if SHELL_CLASSES.contains(&String::from_utf16_lossy(&buf[..n]).as_str()) {
            return BOOL(1);
        }
        let mut r = RECT::default();
        if DwmGetWindowAttribute(hw, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut _ as *mut _, std::mem::size_of::<RECT>() as u32).is_err()
            && GetWindowRect(hw, &mut r).is_err()
        {
            return BOOL(1);
        }
        if r.right - r.left < 150 || r.bottom - r.top < 80 {
            return BOOL(1);
        }
        let _ = ex & WS_EX_TOPMOST.0;
        let mut pid = 0u32;
        GetWindowThreadProcessId(hw, Some(&mut pid));
        out.push(WinInfo { hwnd: hw.0 as isize, rect: r.into(), maximized: IsZoomed(hw).as_bool(), pid });
        BOOL(1)
    }
    let mut data: (isize, Vec<WinInfo>) = (own, Vec::new());
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut data as *mut _ as isize));
    }
    data.1
}

/// Current visible frame of one window (same bounds as `windows()`), None if it is gone / hidden / minimized.
/// Cheap: the brain calls it every tick for the window she or her bed stands on, so they ride a dragged
/// window at frame rate instead of the 10 Hz full window scan.
pub fn frame_rect(hwnd: isize) -> Option<(Rect, bool)> {
    unsafe {
        let hw = h(hwnd);
        if !windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(hw)).as_bool() || !IsWindowVisible(hw).as_bool() || IsIconic(hw).as_bool() {
            return None;
        }
        let mut r = RECT::default();
        if DwmGetWindowAttribute(hw, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut _ as *mut _, std::mem::size_of::<RECT>() as u32).is_err()
            && GetWindowRect(hw, &mut r).is_err()
        {
            return None;
        }
        Some((r.into(), IsZoomed(hw).as_bool()))
    }
}

/// Windows is running a modal move/size loop on this window (title-bar drag).
pub fn in_move_size(hwnd: isize) -> bool {
    unsafe {
        let tid = GetWindowThreadProcessId(h(hwnd), None);
        let mut gi = GUITHREADINFO { cbSize: std::mem::size_of::<GUITHREADINFO>() as u32, ..Default::default() };
        GetGUIThreadInfo(tid, &mut gi).is_ok() && gi.flags.0 & 0x2 != 0
    }
}

/// Physical left button (respects swapped mouse buttons).
pub fn left_button_down() -> bool {
    unsafe {
        let vk = if GetSystemMetrics(SM_SWAPBUTTON) != 0 { 0x02 } else { VK_LBUTTON.0 as i32 };
        (GetAsyncKeyState(vk) as u16 & 0x8000) != 0
    }
}
