//! Henry's voice (PLAN #12, #28, #55): one real meow of the real Henry, cut from a phone video
//! and cleaned (`art/sound/meow.wav`). Played with PlaySound from memory. No synthesized sounds, no purr.
//!
//! She meows rarely and only when it can't get in the way: never while a quiet app (Resolve) is running
//! (not just in front) or while something else is playing sound.
use std::time::Duration;
use windows::core::PCWSTR;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator, PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};

static MEOW: &[u8] = include_bytes!("../../art/sound/meow.wav");

fn play(buf: &'static [u8]) -> bool {
    unsafe { PlaySoundW(PCWSTR(buf.as_ptr() as *const u16), None, SND_MEMORY | SND_ASYNC | SND_NODEFAULT).as_bool() }
}

/// Meow right now (API `/do meow`, tests).
pub fn meow() -> bool {
    play(MEOW)
}

/// Peak level of the default output over ~250 ms (0..1); None if it can't be read.
fn output_peak() -> Option<f32> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let dev = en.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        let meter: IAudioMeterInformation = dev.Activate(CLSCTX_ALL, None).ok()?;
        let mut peak = 0.0f32;
        for _ in 0..8 {
            peak = peak.max(meter.GetPeakValue().ok()?);
            std::thread::sleep(Duration::from_millis(30));
        }
        Some(peak)
    }
}

/// One of these exe names is running at all.
fn running(names: &[String]) -> Option<String> {
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut found = None;
        if Process32FirstW(snap, &mut e).is_ok() {
            loop {
                let n = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
                let exe = String::from_utf16_lossy(&e.szExeFile[..n]);
                if names.iter().any(|q| q.eq_ignore_ascii_case(&exe)) {
                    found = Some(exe);
                    break;
                }
                if Process32NextW(snap, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        found
    }
}

/// Meow unless a quiet app is running or the PC is already playing sound. Runs on its own short thread
/// (the meter check takes ~250 ms); `done` gets what happened, for the log.
pub fn meow_if_calm(quiet_apps: Vec<String>, done: impl FnOnce(String) + Send + 'static) {
    std::thread::spawn(move || {
        let why = if let Some(app) = running(&quiet_apps) {
            format!("meow skipped: {app} is running")
        } else {
            match output_peak() {
                Some(p) if p > 0.02 => format!("meow skipped: sound playing (peak {p:.2})"),
                p => {
                    let ok = meow();
                    format!("meow (played {ok}, output peak {})", p.map_or("?".into(), |p| format!("{p:.3}")))
                }
            }
        };
        done(why);
    });
}
