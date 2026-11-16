//! Update check (PLAN #65): once a day Henry asks GitHub for the latest public release of
//! `exaar/henry_pet`; a newer version puts "Update available" at the top of the tray menu, and clicking it
//! downloads the installer in the browser (the installer goes over the old version, settings stay).
//! Nothing is downloaded or installed by the app itself.
//!
//! - First check ~1 min after start, then every 24 h. The last answer is kept in `update.json` next to
//!   settings.json, so restarts don't ask again and a known update shows right away.
//! - `"update_check": false` in settings.json turns it off (read at start).
//! - Plain HTTPS GET via WinHTTP (system proxy settings), no token, no data sent beyond the request itself.
//! - `GET /update` shows the state, `POST /update` checks now (tests). `HENRY_UPDATE_URL=<url>` points the
//!   check at another releases/latest-shaped JSON (local tests).
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const LATEST: &str = "https://api.github.com/repos/exaar/henry_pet/releases/latest";
const EVERY: u64 = 24 * 3600;
const FIRST_DELAY: Duration = Duration::from_secs(60);

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default)]
pub struct State {
    /// Unix seconds of the last finished check (ok or not).
    pub checked_at: u64,
    /// Newest public version seen ("0.4.0"), if any.
    pub latest: Option<String>,
    /// Installer download (falls back to the release page).
    pub url: Option<String>,
    /// Release page.
    pub page: Option<String>,
    /// Last error, if the last check failed.
    pub error: Option<String>,
}

impl State {
    /// A newer version than this build is out.
    pub fn available(&self) -> Option<&str> {
        let latest = self.latest.as_deref()?;
        (parse(latest)? > parse(env!("CARGO_PKG_VERSION"))?).then_some(latest)
    }
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static WAKE: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());

pub fn state() -> State {
    STATE.lock().unwrap().clone().unwrap_or_default()
}

pub fn status_json() -> Value {
    let s = state();
    serde_json::json!({
        "current": env!("CARGO_PKG_VERSION"),
        "latest": s.latest,
        "available": s.available(),
        "url": s.url,
        "page": s.page,
        "checked_at": s.checked_at,
        "error": s.error,
        "enabled": enabled(),
    })
}

static ENABLED: Mutex<Option<bool>> = Mutex::new(None);

pub fn enabled() -> bool {
    ENABLED.lock().unwrap().unwrap_or(true)
}

/// settings.json `update_check` changed (hand edit): applies live.
pub fn set_enabled(on: bool) {
    let mut e = ENABLED.lock().unwrap();
    if *e != Some(on) {
        *e = Some(on);
        WAKE.1.notify_all();
    }
}

/// Check now (API), even when the daily check is off.
pub fn check_now() {
    let (m, cv) = &WAKE;
    *m.lock().unwrap() = true;
    cv.notify_all();
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// "v0.4.0" / "0.4" -> (0, 4, 0); anything else -> None.
fn parse(v: &str) -> Option<(u32, u32, u32)> {
    let v = v.trim().trim_start_matches(['v', 'V']);
    let v = v.split(['-', '+']).next()?;
    let mut it = v.split('.').map(|p| p.parse::<u32>().ok());
    let a = it.next()??;
    let b = it.next().unwrap_or(Some(0))?;
    let c = it.next().unwrap_or(Some(0))?;
    Some((a, b, c))
}

/// Start the background checker. `on_found` runs (any thread) whenever a check finds an update, and once
/// at start if the cached answer already has one.
pub fn spawn(dir: PathBuf, on: bool, on_found: impl Fn(&State) + Send + 'static) {
    set_enabled(on);
    let file = dir.join("update.json");
    let cached: State = std::fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    *STATE.lock().unwrap() = Some(cached.clone());
    std::thread::spawn(move || {
        if cached.available().is_some() {
            on_found(&cached);
        }
        let url = std::env::var("HENRY_UPDATE_URL").unwrap_or_else(|_| LATEST.to_string());
        let started = now();
        loop {
            // due: a day after the last answer (an hour after a failure), never sooner than a minute after start
            let asked = {
                let (m, cv) = &WAKE;
                let mut g = m.lock().unwrap();
                loop {
                    if *g {
                        break true;
                    }
                    let s = state();
                    let due = (s.checked_at + if s.error.is_some() { 3600 } else { EVERY }).max(started + FIRST_DELAY.as_secs());
                    let t = now();
                    if enabled() && t >= due {
                        break false;
                    }
                    let wait = if enabled() { Duration::from_secs(due - t) } else { Duration::from_secs(3600) };
                    g = cv.wait_timeout(g, wait).unwrap().0;
                }
            };
            if asked {
                *WAKE.0.lock().unwrap() = false;
            }
            let mut s = state();
            match fetch(&url).and_then(|body| body.map(|b| read_release(&b)).transpose()) {
                // 404: no public release yet
                Ok(None) => {
                    (s.latest, s.url, s.page, s.error) = (None, None, None, None);
                }
                Ok(Some((latest, inst, page))) => {
                    s.latest = Some(latest);
                    s.url = inst.or_else(|| page.clone());
                    s.page = page;
                    s.error = None;
                }
                Err(e) => s.error = Some(e),
            }
            s.checked_at = now();
            *STATE.lock().unwrap() = Some(s.clone());
            if let Ok(t) = serde_json::to_string_pretty(&s) {
                let _ = std::fs::write(&file, t);
            }
            if s.available().is_some() {
                on_found(&s);
            }
        }
    });
}

/// (version, installer url, release page) from a GitHub releases/latest answer.
fn read_release(body: &str) -> Result<(String, Option<String>, Option<String>), String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("bad JSON: {e}"))?;
    let tag = v["tag_name"].as_str().ok_or("no tag_name")?;
    parse(tag).ok_or_else(|| format!("unreadable version {tag:?}"))?;
    let latest = tag.trim_start_matches(['v', 'V']).to_string();
    let inst = v["assets"].as_array().into_iter().flatten().filter_map(|a| a["browser_download_url"].as_str()).find(|u| u.ends_with("-setup.exe")).map(str::to_string);
    let page = v["html_url"].as_str().map(str::to_string);
    Ok((latest, inst, page))
}

/// WinHTTP errors have no system message text: name the common ones, else the code.
fn why(e: &windows::core::Error) -> String {
    let code = (e.code().0 as u32) & 0xFFFF;
    match code {
        12002 => "timed out".into(),
        12007 => "name not resolved (offline?)".into(),
        12029 => "cannot connect".into(),
        12030 => "connection dropped".into(),
        12175 => "TLS error".into(),
        _ => {
            let m = e.message();
            if m.is_empty() { format!("WinHTTP error {code}") } else { m }
        }
    }
}

/// GET over WinHTTP: Ok(Some(body)) for HTTP 200, Ok(None) for 404.
fn fetch(url: &str) -> Result<Option<String>, String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Networking::WinHttp::*;

    let (secure, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else {
        return Err(format!("bad url {url}"));
    };
    let (hostport, path) = rest.split_once('/').map(|(h, p)| (h, format!("/{p}"))).unwrap_or((rest, "/".into()));
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().map_err(|_| "bad port")?),
        None => (hostport, if secure { INTERNET_DEFAULT_HTTPS_PORT } else { 80 }),
    };

    struct H(*mut core::ffi::c_void);
    impl Drop for H {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    let _ = WinHttpCloseHandle(self.0);
                }
            }
        }
    }
    let last = || why(&windows::core::Error::from_win32());
    unsafe {
        let agent = HSTRING::from(format!("henry_pet/{}", env!("CARGO_PKG_VERSION")));
        let session = H(WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0));
        if session.0.is_null() {
            return Err(format!("WinHttpOpen: {}", last()));
        }
        let _ = WinHttpSetTimeouts(session.0, 10_000, 10_000, 10_000, 15_000);
        let conn = H(WinHttpConnect(session.0, &HSTRING::from(host), port, 0));
        if conn.0.is_null() {
            return Err(format!("connect {host}: {}", last()));
        }
        let flags = if secure { WINHTTP_FLAG_SECURE } else { WINHTTP_OPEN_REQUEST_FLAGS(0) };
        let req = H(WinHttpOpenRequest(conn.0, &HSTRING::from("GET"), &HSTRING::from(path.as_str()), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), flags));
        if req.0.is_null() {
            return Err(format!("open request: {}", last()));
        }
        let hdr: Vec<u16> = "Accept: application/vnd.github+json\r\nX-GitHub-Api-Version: 2022-11-28\r\n".encode_utf16().collect();
        WinHttpSendRequest(req.0, Some(&hdr), None, 0, 0, 0).map_err(|e| format!("send: {}", why(&e)))?;
        WinHttpReceiveResponse(req.0, std::ptr::null_mut()).map_err(|e| format!("receive: {}", why(&e)))?;
        let mut code: u32 = 0;
        let mut len = 4u32;
        WinHttpQueryHeaders(req.0, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, PCWSTR::null(), Some(&mut code as *mut u32 as *mut _), &mut len, std::ptr::null_mut())
            .map_err(|e| format!("status: {}", why(&e)))?;
        let mut body = Vec::new();
        loop {
            let mut avail = 0u32;
            WinHttpQueryDataAvailable(req.0, &mut avail).map_err(|e| format!("read: {}", why(&e)))?;
            if avail == 0 {
                break;
            }
            let start = body.len();
            body.resize(start + avail as usize, 0);
            let mut got = 0u32;
            WinHttpReadData(req.0, body[start..].as_mut_ptr() as *mut _, avail, &mut got).map_err(|e| format!("read: {}", why(&e)))?;
            body.truncate(start + got as usize);
            if body.len() > 4 << 20 {
                return Err("answer too big".into());
            }
        }
        // 404 = no public release yet: not an error worth retrying hourly
        if code == 404 {
            return Ok(None);
        }
        if code != 200 {
            return Err(format!("HTTP {code}"));
        }
        String::from_utf8(body).map(Some).map_err(|_| "answer is not UTF-8".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions() {
        assert_eq!(parse("v0.4.0"), Some((0, 4, 0)));
        assert_eq!(parse("1.2"), Some((1, 2, 0)));
        assert_eq!(parse("v1.2.3-beta"), Some((1, 2, 3)));
        assert_eq!(parse("nightly"), None);
        assert!(parse("0.10.0") > parse("0.9.9"));
    }
}
