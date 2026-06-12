use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

pub fn unix_timestamp() -> Result<i64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before the Unix epoch")?
        .as_secs()
        .min(i64::MAX as u64) as i64)
}

pub fn timings_enabled() -> bool {
    std::env::var_os("KLOCC_SCAN_TIMINGS").is_some()
}

pub fn log_timing(scope: &str, phase: &str, duration: Duration) {
    if timings_enabled() {
        eprintln!("klocc: timing {scope}.{phase} {:.3}ms", duration.as_secs_f64() * 1000.0);
    }
}
