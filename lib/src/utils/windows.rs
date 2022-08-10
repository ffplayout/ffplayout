use std::{env, ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf, ptr};

use winapi::shared::minwindef::MAX_PATH;
use winapi::shared::winerror::S_OK;
use winapi::um::shlobj::{SHGetFolderPathW, CSIDL_PROFILE};

pub fn home_dir_inner() -> Option<PathBuf> {
    env::var_os("USERPROFILE")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(home_dir_crt)
}

#[cfg(not(target_vendor = "uwp"))]
fn home_dir_crt() -> Option<PathBuf> {
    unsafe {
        let mut path: Vec<u16> = Vec::with_capacity(MAX_PATH);
        match SHGetFolderPathW(
            ptr::null_mut(),
            CSIDL_PROFILE,
            ptr::null_mut(),
            0,
            path.as_mut_ptr(),
        ) {
            S_OK => {
                let len = wcslen(path.as_ptr());
                path.set_len(len);
                let s = OsString::from_wide(&path);
                Some(PathBuf::from(s))
            }
            _ => None,
        }
    }
}

#[cfg(target_vendor = "uwp")]
fn home_dir_crt() -> Option<PathBuf> {
    None
}

extern "C" {
    fn wcslen(buf: *const u16) -> usize;
}
