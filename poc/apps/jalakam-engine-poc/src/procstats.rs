//! Process resource sampling: CPU, memory, handles, threads, and per-process
//! GPU engine utilisation from the documented "GPU Engine" PDH counters.

use std::time::Instant;

use jkp_proto::ProcessStats;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, FILETIME};
use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32};
use windows::Win32::System::Performance::*;
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX};
use windows::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId, GetProcessHandleCount, GetProcessTimes, GetThreadDescription, OpenThread,
    THREAD_QUERY_LIMITED_INFORMATION,
};

pub struct ProcSampler {
    last_cpu_100ns: u64,
    last_wall: Instant,
    cores: u32,
    pid: u32,
    query: isize,
    engine: isize,
    memory: isize,
}

fn ft(f: FILETIME) -> u64 {
    ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64
}

fn cpu_100ns() -> u64 {
    unsafe {
        let (mut c, mut e, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
        let _ = GetProcessTimes(GetCurrentProcess(), &mut c, &mut e, &mut k, &mut u);
        ft(k) + ft(u)
    }
}

pub fn thread_count() -> u32 {
    unsafe {
        let pid = GetCurrentProcessId();
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) else { return 0 };
        let mut te = THREADENTRY32 { dwSize: std::mem::size_of::<THREADENTRY32>() as u32, ..Default::default() };
        let mut n = 0;
        if Thread32First(snap, &mut te).is_ok() {
            loop {
                if te.th32OwnerProcessID == pid {
                    n += 1;
                }
                if Thread32Next(snap, &mut te).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        n
    }
}

/// Names of this process's threads other than the caller. Rust's std names
/// every thread it spawns with a description, so an engine thread that
/// outlived shutdown shows up here; unnamed threads belong to the OS, MF,
/// WASAPI or the GPU driver.
pub fn named_threads() -> (Vec<String>, u32) {
    let mut names = Vec::new();
    let mut unnamed = 0;
    unsafe {
        let pid = GetCurrentProcessId();
        let me = GetCurrentThreadId();
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) else { return (names, 0) };
        let mut te = THREADENTRY32 { dwSize: std::mem::size_of::<THREADENTRY32>() as u32, ..Default::default() };
        if Thread32First(snap, &mut te).is_ok() {
            loop {
                if te.th32OwnerProcessID == pid && te.th32ThreadID != me {
                    let mut name = String::new();
                    if let Ok(h) = OpenThread(THREAD_QUERY_LIMITED_INFORMATION, false, te.th32ThreadID) {
                        if let Ok(d) = GetThreadDescription(h) {
                            name = d.to_string().unwrap_or_default();
                            let _ = windows::Win32::Foundation::LocalFree(Some(windows::Win32::Foundation::HLOCAL(d.0 as *mut _)));
                        }
                        let _ = CloseHandle(h);
                    }
                    if name.is_empty() {
                        unnamed += 1;
                    } else {
                        names.push(name);
                    }
                }
                if Thread32Next(snap, &mut te).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    (names, unnamed)
}

pub fn handle_count() -> u32 {
    let mut n = 0u32;
    unsafe {
        let _ = GetProcessHandleCount(GetCurrentProcess(), &mut n);
    }
    n
}

impl ProcSampler {
    pub fn new() -> Self {
        let mut si = SYSTEM_INFO::default();
        unsafe { GetSystemInfo(&mut si) };
        let mut s = Self {
            last_cpu_100ns: cpu_100ns(),
            last_wall: Instant::now(),
            cores: si.dwNumberOfProcessors,
            pid: unsafe { GetCurrentProcessId() },
            query: 0,
            engine: 0,
            memory: 0,
        };
        unsafe {
            let mut q = PDH_HQUERY::default();
            if PdhOpenQueryW(PCWSTR::null(), 0, &mut q) == 0 {
                let mut c = PDH_HCOUNTER::default();
                let mut m = PDH_HCOUNTER::default();
                let ok1 = PdhAddEnglishCounterW(q, w!("\\GPU Engine(*)\\Utilization Percentage"), 0, &mut c) == 0;
                let ok2 = PdhAddEnglishCounterW(q, w!("\\GPU Process Memory(*)\\Dedicated Usage"), 0, &mut m) == 0;
                s.query = q.0 as isize;
                if ok1 {
                    s.engine = c.0 as isize;
                }
                if ok2 {
                    s.memory = m.0 as isize;
                }
                PdhCollectQueryData(q);
            }
        }
        s
    }

    unsafe fn read_array(&self, counter: isize) -> Vec<(String, f64)> {
        let mut out = Vec::new();
        if counter == 0 {
            return out;
        }
        let c = PDH_HCOUNTER(counter as *mut _);
        let (mut size, mut count) = (0u32, 0u32);
        let _ = PdhGetFormattedCounterArrayW(c, PDH_FMT_DOUBLE, &mut size, &mut count, None);
        if size == 0 {
            return out;
        }
        let mut buf = vec![0u8; size as usize];
        if PdhGetFormattedCounterArrayW(c, PDH_FMT_DOUBLE, &mut size, &mut count, Some(buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W)) != 0 {
            return out;
        }
        let items = std::slice::from_raw_parts(buf.as_ptr() as *const PDH_FMT_COUNTERVALUE_ITEM_W, count as usize);
        for it in items {
            let name = it.szName.to_string().unwrap_or_default();
            out.push((name, it.FmtValue.Anonymous.doubleValue));
        }
        out
    }

    pub fn sample(&mut self) -> ProcessStats {
        let now_cpu = cpu_100ns();
        let wall = self.last_wall.elapsed().as_secs_f64().max(1e-3);
        let cpu_s = (now_cpu - self.last_cpu_100ns) as f64 / 1e7;
        self.last_cpu_100ns = now_cpu;
        self.last_wall = Instant::now();
        let mut st = ProcessStats {
            cpu_pct_of_one_core: cpu_s / wall * 100.0,
            cpu_pct_of_machine: cpu_s / wall * 100.0 / self.cores as f64,
            handles: handle_count(),
            threads: thread_count(),
            ..Default::default()
        };
        unsafe {
            let mut pmc = PROCESS_MEMORY_COUNTERS_EX { cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32, ..Default::default() };
            if GetProcessMemoryInfo(GetCurrentProcess(), &mut pmc as *mut _ as *mut PROCESS_MEMORY_COUNTERS, pmc.cb).is_ok() {
                st.working_set_mb = pmc.WorkingSetSize as f64 / 1048576.0;
                st.private_mb = pmc.PrivateUsage as f64 / 1048576.0;
            }
            if self.query != 0 {
                PdhCollectQueryData(PDH_HQUERY(self.query as *mut _));
                let prefix = format!("pid_{}_", self.pid);
                for (name, v) in self.read_array(self.engine) {
                    if !name.starts_with(&prefix) {
                        continue;
                    }
                    if name.ends_with("engtype_3D") {
                        st.gpu_3d_pct += v;
                    } else if name.contains("engtype_VideoEncode") {
                        st.gpu_video_encode_pct += v;
                    } else if name.contains("engtype_VideoProcessing") {
                        st.gpu_video_processing_pct += v;
                    } else if name.ends_with("engtype_Copy") {
                        st.gpu_copy_pct += v;
                    }
                }
                for (name, v) in self.read_array(self.memory) {
                    if name.starts_with(&prefix) {
                        st.gpu_dedicated_mb += v / 1048576.0;
                    }
                }
            }
        }
        st
    }
}

impl Drop for ProcSampler {
    fn drop(&mut self) {
        if self.query != 0 {
            unsafe {
                PdhCloseQuery(PDH_HQUERY(self.query as *mut _));
            }
        }
    }
}
