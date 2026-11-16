//! Desktop icon mischief (PLAN #10): read and move icons of the desktop's SysListView32 (Explorer).
//!
//! Positions are read through a small buffer allocated inside Explorer (list-view messages that return
//! data need memory in the owner process); moves use LVM_SETITEMPOSITION, which needs none. Every message
//! goes through SendMessageTimeout(ABORTIFHUNG) so a hung Explorer can never freeze Henry.
//! Original positions of every icon she moves are kept in icons.json until "Return icons".
use crate::win::Rect;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::path::Path;
use windows::core::{w, BOOL, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};
use windows::Win32::System::Memory::{VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, GetWindowLongPtrW, GetWindowThreadProcessId, IsWindowVisible, SendMessageTimeoutW,
    GWL_STYLE, SMTO_ABORTIFHUNG,
};

const LVM_GETITEMCOUNT: u32 = 0x1004;
const LVM_GETITEMRECT: u32 = 0x100E;
const LVM_SETITEMPOSITION: u32 = 0x100F;
const LVM_GETITEMPOSITION: u32 = 0x1010;
const LVM_GETEXTENDEDLISTVIEWSTYLE: u32 = 0x1037;
const LVM_GETITEMTEXTW: u32 = 0x1073;
const LVS_AUTOARRANGE: isize = 0x0100;
const LVS_EX_SNAPTOGRID: usize = 0x0008_0000;

#[repr(C)]
struct LvItemW {
    mask: u32,
    i_item: i32,
    i_sub_item: i32,
    state: u32,
    state_mask: u32,
    psz_text: *mut u16,
    cch_text_max: i32,
    i_image: i32,
    l_param: isize,
    i_indent: i32,
    i_group_id: i32,
    c_columns: u32,
    pu_columns: *mut u32,
    pi_col_fmt: *mut i32,
    i_group: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Icon {
    pub index: i32,
    pub name: String,
    /// List-view client position (what LVM_SETITEMPOSITION takes).
    pub pos: (i32, i32),
    /// Screen bounds.
    pub rect: Rect,
}

fn hwnd(v: isize) -> HWND {
    HWND(v as *mut c_void)
}

fn send(lv: isize, msg: u32, w: usize, l: isize) -> Option<usize> {
    let mut out = 0usize;
    let r = unsafe { SendMessageTimeoutW(hwnd(lv), msg, WPARAM(w), LPARAM(l), SMTO_ABORTIFHUNG, 300, Some(&mut out)) };
    (r.0 != 0).then_some(out)
}

struct Remote {
    lv: isize,
    proc: HANDLE,
    mem: *mut c_void,
}

impl Remote {
    fn open(lv: isize) -> Option<Self> {
        unsafe {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd(lv), Some(&mut pid));
            if pid == 0 {
                return None;
            }
            let proc = OpenProcess(
                PROCESS_VM_OPERATION | PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_QUERY_LIMITED_INFORMATION,
                false,
                pid,
            )
            .ok()?;
            let mem = VirtualAllocEx(proc, None, 4096, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
            if mem.is_null() {
                let _ = CloseHandle(proc);
                return None;
            }
            Some(Remote { lv, proc, mem })
        }
    }
    fn at(&self, off: usize) -> *mut c_void {
        (self.mem as usize + off) as *mut c_void
    }
    fn write<T>(&self, off: usize, v: &T) -> bool {
        unsafe { WriteProcessMemory(self.proc, self.at(off), v as *const T as *const c_void, std::mem::size_of::<T>(), None).is_ok() }
    }
    fn read_into(&self, off: usize, dst: *mut c_void, len: usize) -> bool {
        unsafe { ReadProcessMemory(self.proc, self.at(off), dst, len, None).is_ok() }
    }
    fn read<T: Default>(&self, off: usize) -> Option<T> {
        let mut v = T::default();
        self.read_into(off, &mut v as *mut T as *mut c_void, std::mem::size_of::<T>()).then_some(v)
    }
    fn send(&self, msg: u32, w: usize, l: isize) -> Option<usize> {
        send(self.lv, msg, w, l)
    }
}

impl Drop for Remote {
    fn drop(&mut self) {
        unsafe {
            let _ = VirtualFreeEx(self.proc, self.mem, 0, MEM_RELEASE);
            let _ = CloseHandle(self.proc);
        }
    }
}

/// The desktop list view (Progman or WorkerW > SHELLDLL_DefView > SysListView32).
/// Dev knob: HENRY_ICON_LV=<hwnd> points her at a test list view instead.
pub fn desktop_listview() -> Option<isize> {
    if let Some(v) = std::env::var("HENRY_ICON_LV").ok().and_then(|v| v.trim().parse::<isize>().ok()) {
        return Some(v);
    }
    unsafe extern "system" fn cb(top: HWND, data: LPARAM) -> BOOL {
        let out = &mut *(data.0 as *mut isize);
        if let Ok(def) = FindWindowExW(Some(top), None, w!("SHELLDLL_DefView"), PCWSTR::null()) {
            if let Ok(lv) = FindWindowExW(Some(def), None, w!("SysListView32"), PCWSTR::null()) {
                *out = lv.0 as isize;
                return BOOL(0);
            }
        }
        BOOL(1)
    }
    let mut out = 0isize;
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut out as *mut _ as isize));
    }
    (out != 0).then_some(out)
}

/// Why she must not touch the icons right now, if anything.
pub fn blocked(lv: isize) -> Option<&'static str> {
    unsafe {
        if !IsWindowVisible(hwnd(lv)).as_bool() {
            return Some("desktop icons are hidden");
        }
        if GetWindowLongPtrW(hwnd(lv), GWL_STYLE) & LVS_AUTOARRANGE != 0 {
            return Some("auto-arrange is on");
        }
    }
    match send(lv, LVM_GETEXTENDEDLISTVIEWSTYLE, 0, 0) {
        Some(ex) if ex & LVS_EX_SNAPTOGRID != 0 => Some("align-to-grid is on"),
        Some(_) => None,
        None => Some("explorer is not answering"),
    }
}

fn origin(lv: isize) -> (i32, i32) {
    let mut p = POINT::default();
    unsafe {
        let _ = ClientToScreen(hwnd(lv), &mut p);
    }
    (p.x, p.y)
}

pub fn list(lv: isize) -> Option<Vec<Icon>> {
    let r = Remote::open(lv)?;
    let n = r.send(LVM_GETITEMCOUNT, 0, 0)? as i32;
    let (ox, oy) = origin(lv);
    let mut out = Vec::new();
    for i in 0..n.min(400) {
        r.send(LVM_GETITEMPOSITION, i as usize, r.at(0) as isize)?;
        let p: POINT = r.read(0)?;
        r.write(16, &RECT::default()); // left = LVIR_BOUNDS (0)
        r.send(LVM_GETITEMRECT, i as usize, r.at(16) as isize)?;
        let rc: RECT = r.read(16)?;
        let mut item: LvItemW = unsafe { std::mem::zeroed() };
        item.psz_text = r.at(256) as *mut u16;
        item.cch_text_max = 260;
        r.write(64, &item);
        let len = r.send(LVM_GETITEMTEXTW, i as usize, r.at(64) as isize).unwrap_or(0).min(259);
        let mut buf = vec![0u16; 260];
        r.read_into(256, buf.as_mut_ptr() as *mut c_void, 520);
        out.push(Icon {
            index: i,
            name: String::from_utf16_lossy(&buf[..len]),
            pos: (p.x, p.y),
            rect: Rect { left: rc.left + ox, top: rc.top + oy, right: rc.right + ox, bottom: rc.bottom + oy },
        });
    }
    Some(out)
}

pub fn set_pos(lv: isize, index: i32, x: i32, y: i32) -> bool {
    if x < 0 || y < 0 {
        return false;
    }
    let l = ((y as u16 as isize) << 16) | (x as u16 as isize);
    send(lv, LVM_SETITEMPOSITION, index as usize, l).is_some()
}

#[derive(Serialize, Deserialize, Default)]
pub struct Store {
    /// Original positions of icons she moved, by name.
    pub moved: BTreeMap<String, (i32, i32)>,
    /// Every icon's position before her first move ever (safety net).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<BTreeMap<String, (i32, i32)>>,
}

pub fn load(path: &Path) -> Store {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save(path: &Path, s: &Store) {
    if let Ok(t) = serde_json::to_string_pretty(s) {
        let _ = std::fs::write(path, t);
    }
}

/// Remember where the icon was before her first move of it (keeps the oldest original).
pub fn record_original(path: &Path, name: &str, pos: (i32, i32), all: &[Icon]) {
    let mut s = load(path);
    if s.snapshot.is_none() {
        s.snapshot = Some(all.iter().map(|i| (i.name.clone(), i.pos)).collect());
    }
    s.moved.entry(name.to_string()).or_insert(pos);
    save(path, &s);
}

/// Put every moved icon back. Returns how many were returned.
pub fn return_all(path: &Path) -> Result<usize, String> {
    let mut s = load(path);
    if s.moved.is_empty() {
        return Ok(0);
    }
    let lv = desktop_listview().ok_or("no desktop list view")?;
    let icons = list(lv).ok_or("could not read icons")?;
    let mut n = 0;
    for (name, (x, y)) in &s.moved {
        if let Some(ic) = icons.iter().find(|i| &i.name == name) {
            if set_pos(lv, ic.index, *x, *y) {
                n += 1;
            }
        }
    }
    s.moved.clear();
    save(path, &s);
    Ok(n)
}
