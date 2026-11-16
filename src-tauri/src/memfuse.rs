//! Memory fuse: Henry must never take the machine down. Every 3 s sum her private memory (and any child
//! processes); above the limit, write fuse.log and exit.
use crate::brain::Control;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX};
use windows::Win32::System::Threading::{GetCurrentProcessId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

const LIMIT_MB: u64 = 500;

fn descendants(root: u32) -> Vec<u32> {
    let mut parent: HashMap<u32, u32> = HashMap::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return vec![] };
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        if Process32FirstW(snap, &mut e).is_ok() {
            loop {
                parent.insert(e.th32ProcessID, e.th32ParentProcessID);
                if Process32NextW(snap, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    let mut set: HashSet<u32> = HashSet::from([root]);
    loop {
        let before = set.len();
        for (pid, ppid) in &parent {
            if set.contains(ppid) && *pid != 0 {
                set.insert(*pid);
            }
        }
        if set.len() == before {
            break;
        }
    }
    set.remove(&root);
    set.into_iter().collect()
}

fn private_mb(pid: u32) -> u64 {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return 0 };
        let mut c = PROCESS_MEMORY_COUNTERS_EX { cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32, ..Default::default() };
        let ok = GetProcessMemoryInfo(h, &mut c as *mut _ as *mut PROCESS_MEMORY_COUNTERS, c.cb).is_ok();
        let _ = CloseHandle(h);
        if ok { c.PrivateUsage as u64 / (1024 * 1024) } else { 0 }
    }
}

pub fn spawn(dir: PathBuf, ctl: Arc<Mutex<Control>>) {
    std::thread::spawn(move || {
        let me = unsafe { GetCurrentProcessId() };
        loop {
            std::thread::sleep(Duration::from_secs(3));
            let mb: u64 = private_mb(me) + descendants(me).into_iter().map(private_mb).sum::<u64>();
            let recent = {
                let mut c = ctl.lock().unwrap();
                c.webview_mb = mb;
                c.log.iter().cloned().collect::<Vec<_>>()
            };
            if mb > LIMIT_MB {
                let msg = format!("webview tree at {mb} MB > {LIMIT_MB} MB: exiting.\nrecent:\n{}\n", recent.join("\n"));
                let _ = std::fs::write(dir.join("fuse.log"), msg);
                std::process::exit(3);
            }
        }
    });
}
