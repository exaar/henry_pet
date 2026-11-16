//! Per-process GPU load (M6 "warm spot", PLAN #52) from the `GPU Engine` performance counters - the same
//! numbers Task Manager shows. Sampled on its own thread every 2 s so the brain never waits on PDH.
//!
//! Load of a process = its busiest engine type (3D, Compute, VideoEncode, ...), summed over that type's
//! engines and adapters, like Task Manager's GPU column.
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows::core::{w, PCWSTR};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW,
    PDH_CSTATUS_VALID_DATA, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
};

pub const PERIOD: Duration = Duration::from_secs(2);
/// Samples kept per process (30 s).
const KEEP: usize = 15;

#[derive(Default)]
pub struct Gpu {
    /// pid -> recent loads in % (newest last), only processes that used the GPU lately.
    pub hist: HashMap<u32, VecDeque<f64>>,
    pub ok: bool,
}

impl Gpu {
    /// Mean load of the last `n` samples (missing samples count as 0: a process that went quiet cools down).
    pub fn mean(&self, pid: u32, n: usize) -> f64 {
        let Some(h) = self.hist.get(&pid) else { return 0.0 };
        h.iter().rev().take(n).sum::<f64>() / n as f64
    }
}

pub fn spawn() -> Arc<Mutex<Gpu>> {
    let gpu = Arc::new(Mutex::new(Gpu::default()));
    let g = gpu.clone();
    std::thread::spawn(move || run(g));
    gpu
}

struct Query {
    q: PDH_HQUERY,
    c: PDH_HCOUNTER,
}

impl Query {
    fn open() -> Option<Self> {
        unsafe {
            let mut q = PDH_HQUERY::default();
            if PdhOpenQueryW(PCWSTR::null(), 0, &mut q) != 0 {
                return None;
            }
            let mut c = PDH_HCOUNTER::default();
            if PdhAddEnglishCounterW(q, w!("\\GPU Engine(*)\\Utilization Percentage"), 0, &mut c) != 0 {
                PdhCloseQuery(q);
                return None;
            }
            PdhCollectQueryData(q); // rate counter: the first collect only sets the baseline
            Some(Query { q, c })
        }
    }

    /// pid -> load % since the previous collect.
    fn sample(&self) -> Option<HashMap<u32, f64>> {
        unsafe {
            if PdhCollectQueryData(self.q) != 0 {
                return None;
            }
            let (mut size, mut count) = (0u32, 0u32);
            let r = PdhGetFormattedCounterArrayW(self.c, PDH_FMT_DOUBLE, &mut size, &mut count, None);
            if r != PDH_MORE_DATA || size == 0 {
                return Some(HashMap::new());
            }
            // u64 storage keeps the item array aligned; the names live in the same buffer
            let mut buf = vec![0u64; (size as usize).div_ceil(8)];
            let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
            if PdhGetFormattedCounterArrayW(self.c, PDH_FMT_DOUBLE, &mut size, &mut count, Some(items)) != 0 {
                return None;
            }
            // (pid, engine type) -> summed load
            let mut by_type: HashMap<(u32, String), f64> = HashMap::new();
            for i in 0..count as usize {
                let it = &*items.add(i);
                if it.FmtValue.CStatus != PDH_CSTATUS_VALID_DATA {
                    continue;
                }
                let v = it.FmtValue.Anonymous.doubleValue;
                if v <= 0.0 {
                    continue;
                }
                let name = it.szName.to_string().unwrap_or_default();
                if let Some(key) = parse(&name) {
                    *by_type.entry(key).or_default() += v;
                }
            }
            let mut out: HashMap<u32, f64> = HashMap::new();
            for ((pid, _), v) in by_type {
                let e = out.entry(pid).or_default();
                *e = e.max(v.min(100.0));
            }
            Some(out)
        }
    }
}

impl Drop for Query {
    fn drop(&mut self) {
        unsafe {
            PdhCloseQuery(self.q);
        }
    }
}

/// "pid_1234_luid_0x0_0x104FA_phys_0_eng_0_engtype_3D" -> (1234, "3D")
fn parse(name: &str) -> Option<(u32, String)> {
    let rest = name.strip_prefix("pid_")?;
    let pid: u32 = rest.split('_').next()?.parse().ok()?;
    let ty = name.rsplit_once("engtype_")?.1.to_string();
    Some((pid, ty))
}

fn run(gpu: Arc<Mutex<Gpu>>) {
    let mut query: Option<Query> = None;
    let mut opened = std::time::Instant::now();
    loop {
        std::thread::sleep(PERIOD);
        // reopen now and then: new GPU processes (a render started) must show up in the wildcard
        if query.is_none() || opened.elapsed() > Duration::from_secs(30) {
            query = Query::open();
            opened = std::time::Instant::now();
            if query.is_some() {
                std::thread::sleep(Duration::from_millis(500));
            }
        }
        let sample = query.as_ref().and_then(|q| q.sample());
        let mut g = gpu.lock().unwrap();
        g.ok = sample.is_some();
        let Some(s) = sample else {
            query = None;
            continue;
        };
        for (pid, h) in g.hist.iter_mut() {
            h.push_back(s.get(pid).copied().unwrap_or(0.0));
            while h.len() > KEEP {
                h.pop_front();
            }
        }
        for (pid, v) in &s {
            g.hist.entry(*pid).or_insert_with(|| VecDeque::from([*v]));
        }
        g.hist.retain(|_, h| h.iter().any(|v| *v > 0.5));
    }
}
