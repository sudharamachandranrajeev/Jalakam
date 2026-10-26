//! QPC master clock and logging.

use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

#[derive(Clone, Copy)]
pub struct Clock {
    t0: i64,
    freq: i64,
}

impl Clock {
    pub fn start() -> Self {
        let (mut t0, mut freq) = (0i64, 0i64);
        unsafe {
            let _ = QueryPerformanceCounter(&mut t0);
            let _ = QueryPerformanceFrequency(&mut freq);
        }
        Self { t0, freq }
    }
    pub fn now_us(&self) -> i64 {
        let mut t = 0i64;
        unsafe {
            let _ = QueryPerformanceCounter(&mut t);
        }
        ((t - self.t0) as i128 * 1_000_000 / self.freq as i128) as i64
    }
}

pub static LOG_START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {{
        let t = $crate::clock::LOG_START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64();
        let th = std::thread::current();
        eprintln!("[{:9.3}] [{}] {}", t, th.name().unwrap_or("?"), format!($($arg)*));
    }};
}
