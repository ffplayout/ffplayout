//! Aggregate CPU consumed by this process, including all Rayon workers.
use anyhow::Result;

#[cfg(unix)]
pub fn process_cpu_seconds() -> Result<f64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: usage points to writable storage of the required size. RUSAGE_SELF
    // samples this process and its threads; it does not access other processes.
    let result = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if result != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: a successful getrusage call initializes the output structure.
    let usage = unsafe { usage.assume_init() };

    Ok(usage.ru_utime.tv_sec as f64
        + usage.ru_utime.tv_usec as f64 / 1_000_000.0
        + usage.ru_stime.tv_sec as f64
        + usage.ru_stime.tv_usec as f64 / 1_000_000.0)
}

#[cfg(not(unix))]
pub fn process_cpu_seconds() -> Result<f64> {
    anyhow::bail!("process CPU comparison currently requires macOS/Linux/Unix")
}
