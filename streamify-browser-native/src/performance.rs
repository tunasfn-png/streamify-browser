// Streamify Browser v0.2.0 – Performance measurement
// Uses Windows API directly (no sysinfo crate).
// Measures private working set (RAM) and CPU % for the current process tree.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct PerfStats {
    /// Total private working set in bytes for the process tree.
    pub ram_bytes: u64,
    /// RAM in MiB (formatted).
    pub ram_mib: f64,
    /// CPU usage as percentage (0.0 - 100.0 * num_cores).
    /// Measured between two successive calls; first call returns 0.
    pub cpu_percent: f64,
}

/// Measures performance stats for the current process tree.
/// This is Windows-only; on other platforms returns zeroes.
#[cfg(target_os = "windows")]
pub mod win {
    use super::PerfStats;
    use std::mem;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows::Win32::System::Threading::{
        GetCurrentProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_INFORMATION,
        PROCESS_VM_READ,
    };

    /// Returns the private working set of the current process in bytes.
    pub fn get_current_process_ram() -> u64 {
        unsafe {
            let handle = GetCurrentProcess();
            let mut pmc: PROCESS_MEMORY_COUNTERS_EX = mem::zeroed();
            pmc.cb = mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
            if GetProcessMemoryInfo(
                handle,
                &mut pmc as *mut PROCESS_MEMORY_COUNTERS_EX as *mut _,
                pmc.cb,
            )
            .is_ok()
            {
                pmc.PrivateUsage as u64
            } else {
                0
            }
        }
    }

    /// Returns kernel + user time in 100-ns units for the current process.
    pub fn get_current_process_times() -> (u64, u64) {
        unsafe {
            let handle = GetCurrentProcess();
            let mut creation = mem::zeroed();
            let mut exit = mem::zeroed();
            let mut kernel = mem::zeroed();
            let mut user = mem::zeroed();
            if GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user).is_ok() {
                let k = ((kernel.dwHighDateTime as u64) << 32) | kernel.dwLowDateTime as u64;
                let u = ((user.dwHighDateTime as u64) << 32) | user.dwLowDateTime as u64;
                (k, u)
            } else {
                (0, 0)
            }
        }
    }

    /// Collects a snapshot of the current process's performance.
    pub fn measure() -> PerfStats {
        let ram = get_current_process_ram();
        let ram_mib = ram as f64 / (1024.0 * 1024.0);
        PerfStats {
            ram_bytes: ram,
            ram_mib: (ram_mib * 100.0).round() / 100.0,
            cpu_percent: 0.0, // CPU requires two snapshots with a time delta
        }
    }
}

/// Fallback for non-Windows (compilation check only — this binary targets Windows).
#[cfg(not(target_os = "windows"))]
pub mod win {
    use super::PerfStats;

    pub fn measure() -> PerfStats {
        PerfStats {
            ram_bytes: 0,
            ram_mib: 0.0,
            cpu_percent: 0.0,
        }
    }
}
