//! TEMPORARY: logs the game's memory use every 10 seconds (and the system's free memory), to see whether anything of
//! ours leaks (the Paint screen's own entries, the HUD rewrite) and how much headroom there was if the game crashes
//! again (one crash in a Scaleform allocation, 2026-11-18). The other log lines say what was going on at the time.

use std::time::{Duration, Instant};

use crate::log;

#[repr(C)]
#[derive(Default)]
struct ProcessMemoryCounters {
    cb: u32,
    page_fault_count: u32,
    peak_working_set: usize,
    working_set: usize,
    quota_peak_paged_pool: usize,
    quota_paged_pool: usize,
    quota_peak_non_paged_pool: usize,
    quota_non_paged_pool: usize,
    pagefile_usage: usize,
    peak_pagefile_usage: usize,
    private_usage: usize,
}

#[repr(C)]
#[derive(Default)]
struct MemoryStatus {
    length: u32,
    memory_load: u32,
    total_physical: u64,
    available_physical: u64,
    total_page_file: u64,
    available_page_file: u64,
    total_virtual: u64,
    available_virtual: u64,
    available_extended_virtual: u64,
}

unsafe extern "system" {
    fn GetCurrentProcess() -> isize;
    fn K32GetProcessMemoryInfo(process: isize, counters: *mut ProcessMemoryCounters, size: u32) -> i32;
    fn GlobalMemoryStatusEx(status: *mut MemoryStatus) -> i32;
}

const GB: f64 = (1u64 << 30) as f64;

pub(crate) fn start() {
    std::thread::spawn(|| {
        let started = Instant::now();
        loop {
            let mut process = ProcessMemoryCounters { cb: size_of::<ProcessMemoryCounters>() as u32, ..Default::default() };
            let mut system = MemoryStatus { length: size_of::<MemoryStatus>() as u32, ..Default::default() };
            let ok = unsafe {
                K32GetProcessMemoryInfo(GetCurrentProcess(), &mut process, process.cb) != 0
                    && GlobalMemoryStatusEx(&mut system) != 0
            };
            if ok {
                log(&format!(
                    "memory {:>5}s: private {:.2} GB (peak commit {:.2}), working set {:.2} GB; system free: {:.2} GB RAM, {:.2} GB commit",
                    started.elapsed().as_secs(),
                    process.private_usage as f64 / GB,
                    process.peak_pagefile_usage as f64 / GB,
                    process.working_set as f64 / GB,
                    system.available_physical as f64 / GB,
                    system.available_page_file as f64 / GB,
                ));
            }
            std::thread::sleep(Duration::from_secs(10));
        }
    });
}
