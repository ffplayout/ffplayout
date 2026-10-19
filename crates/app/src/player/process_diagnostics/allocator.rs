//! glibc allocator accounting, independent of resident process memory.

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct AllocatorMemory {
    pub arena: Option<u64>,
    pub used: Option<u64>,
    pub free: Option<u64>,
    pub mmap: Option<u64>,
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod glibc {
    use std::{ffi::CStr, mem, sync::OnceLock};

    use nix::libc;

    use super::AllocatorMemory;

    type Mallinfo2 = unsafe extern "C" fn() -> libc::mallinfo2;

    fn resolve(name: &CStr) -> Option<Mallinfo2> {
        // Resolve lazily rather than linking mallinfo2 (introduced in glibc
        // 2.33), so older glibc installations can still start the application.
        let symbol = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };
        if symbol.is_null() {
            return None;
        }

        // The named glibc symbol has this ABI and remains loaded for the
        // process lifetime. No library handle is opened or closed here.
        Some(unsafe { mem::transmute::<*mut libc::c_void, Mallinfo2>(symbol) })
    }

    pub(super) fn sample() -> AllocatorMemory {
        static MALLINFO2: OnceLock<Option<Mallinfo2>> = OnceLock::new();

        let Some(mallinfo2) = MALLINFO2.get_or_init(|| resolve(c"mallinfo2")) else {
            return AllocatorMemory::default();
        };
        // glibc locks its arenas while reading the counters. This neither
        // changes malloc tuning nor attempts to trim the allocator.
        let info = unsafe { mallinfo2() };

        AllocatorMemory {
            arena: Some(info.arena as u64),
            used: Some(info.uordblks as u64),
            free: Some(info.fordblks as u64),
            mmap: Some(info.hblkhd as u64),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{resolve, sample};

        #[test]
        fn missing_allocator_symbol_is_unavailable() {
            assert!(resolve(c"ffplayout_missing_allocator_diagnostic_symbol").is_none());
        }

        #[test]
        fn glibc_sample_accounts_for_used_and_free_arena_bytes() {
            let snapshot = sample();
            if let Some(arena) = snapshot.arena {
                assert_eq!(arena, snapshot.used.unwrap() + snapshot.free.unwrap());
                assert!(snapshot.mmap.is_some());
            } else {
                // Older glibc and fully static builds may not export mallinfo2.
                assert_eq!(snapshot, super::AllocatorMemory::default());
            }
        }
    }
}

pub(super) fn sample() -> AllocatorMemory {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        glibc::sample()
    }
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    {
        AllocatorMemory::default()
    }
}

#[cfg(all(test, not(all(target_os = "linux", target_env = "gnu"))))]
mod tests {
    use super::{AllocatorMemory, sample};

    #[test]
    fn unsupported_allocator_has_no_fabricated_zero_measurements() {
        assert_eq!(sample(), AllocatorMemory::default());
        assert!(sample().used.is_none());
    }
}
