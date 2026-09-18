use std::io;

#[cfg(target_os = "macos")]
pub(super) fn current_rss_bytes() -> io::Result<u64> {
    let mut info = std::mem::MaybeUninit::<libc::proc_taskinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
    let written = unsafe {
        libc::proc_pidinfo(
            std::process::id() as libc::c_int,
            libc::PROC_PIDTASKINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if written <= 0 {
        return Err(io::Error::last_os_error());
    }
    if written != size {
        return Err(io::Error::other("short proc_pidinfo read"));
    }
    Ok(unsafe { info.assume_init() }.pti_resident_size)
}

#[cfg(target_os = "linux")]
pub(super) fn current_rss_bytes() -> io::Result<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm")?;
    resident_bytes(&statm, unsafe { libc::sysconf(libc::_SC_PAGESIZE) })
}

#[cfg(any(target_os = "linux", test))]
fn resident_bytes(statm: &str, page_size: libc::c_long) -> io::Result<u64> {
    let resident_pages = statm
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| io::Error::other("unparseable /proc/self/statm"))?;
    let page_size = u64::try_from(page_size)
        .ok()
        .filter(|size| *size > 0)
        .ok_or_else(|| io::Error::other("invalid page size"))?;
    resident_pages
        .checked_mul(page_size)
        .ok_or_else(|| io::Error::other("current RSS overflowed bytes"))
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
pub(super) fn current_rss_bytes() -> io::Result<u64> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "current RSS sampling is unsupported",
    ))
}

#[cfg(unix)]
pub(super) fn peak_rss_bytes() -> io::Result<u64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    let result = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    let max_rss = unsafe { usage.assume_init() }.ru_maxrss;
    let max_rss = u64::try_from(max_rss)
        .map_err(|_| io::Error::other("getrusage returned a negative peak RSS"))?;

    // macOS reports ru_maxrss in bytes; other Unixes report kibibytes.
    #[cfg(target_os = "macos")]
    return Ok(max_rss);

    #[cfg(not(target_os = "macos"))]
    max_rss
        .checked_mul(1024)
        .ok_or_else(|| io::Error::other("peak RSS overflowed bytes"))
}

#[cfg(windows)]
#[repr(C)]
struct ProcessMemoryCounters {
    cb: u32,
    page_fault_count: u32,
    peak_working_set_size: usize,
    working_set_size: usize,
    quota_peak_paged_pool_usage: usize,
    quota_paged_pool_usage: usize,
    quota_peak_non_paged_pool_usage: usize,
    quota_non_paged_pool_usage: usize,
    pagefile_usage: usize,
    peak_pagefile_usage: usize,
}

#[cfg(windows)]
pub(super) fn peak_rss_bytes() -> io::Result<u64> {
    Ok(process_memory_counters()?.peak_working_set_size as u64)
}

#[cfg(windows)]
pub(super) fn current_rss_bytes() -> io::Result<u64> {
    Ok(process_memory_counters()?.working_set_size as u64)
}

#[cfg(windows)]
fn process_memory_counters() -> io::Result<ProcessMemoryCounters> {
    type Handle = *mut std::ffi::c_void;

    unsafe extern "system" {
        fn GetCurrentProcess() -> Handle;
    }

    #[link(name = "psapi")]
    unsafe extern "system" {
        fn GetProcessMemoryInfo(
            process: Handle,
            counters: *mut ProcessMemoryCounters,
            size: u32,
        ) -> i32;
    }

    let mut counters = ProcessMemoryCounters {
        cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
        page_fault_count: 0,
        peak_working_set_size: 0,
        working_set_size: 0,
        quota_peak_paged_pool_usage: 0,
        quota_paged_pool_usage: 0,
        quota_peak_non_paged_pool_usage: 0,
        quota_non_paged_pool_usage: 0,
        pagefile_usage: 0,
        peak_pagefile_usage: 0,
    };
    let result = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters,
            std::mem::size_of::<ProcessMemoryCounters>() as u32,
        )
    };
    if result == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(counters)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resident_memory_uses_resident_pages_and_the_runtime_page_size() {
        assert_eq!(
            resident_bytes("1000 42 10 1 0 100 0\n", 4096).unwrap(),
            172_032
        );
        assert_eq!(resident_bytes("1000 42", 16_384).unwrap(), 688_128);
        assert_eq!(resident_bytes("1000 0", 4096).unwrap(), 0);
    }

    #[test]
    fn resident_memory_rejects_malformed_counts_and_overflow() {
        for input in [
            "",
            "1000",
            "1000 invalid",
            "1000 -1",
            "1000 18446744073709551615",
        ] {
            assert!(resident_bytes(input, 4096).is_err(), "{input}");
        }
        for page_size in [-1, 0] {
            assert!(resident_bytes("1000 42", page_size).is_err());
        }
    }

    #[test]
    fn current_and_peak_rss_samples_report_nonzero_memory() {
        assert!(current_rss_bytes().expect("current RSS should be readable") > 0);
        assert!(peak_rss_bytes().expect("peak RSS should be readable") > 0);
    }
}
