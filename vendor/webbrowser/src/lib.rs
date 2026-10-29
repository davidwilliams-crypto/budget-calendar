//! Opens HTTP(S) URLs with the OS default handler without a command interpreter.
use std::io;

pub fn open(url: &str) -> io::Result<()> {
    // The URL parser strips ASCII tabs/newlines and trailing controls. Reject them before
    // parsing, so no caller can smuggle a different target through normalization or a NUL.
    if url.chars().any(char::is_control) { return Err(invalid_url()); }
    let url = url.trim();
    let (scheme, rest) = url.split_once("://").ok_or_else(invalid_url)?;
    let authority = rest.split(['/', '?', '#', '\\']).next().unwrap_or_default();
    if !(scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")) || authority.is_empty() || authority.contains('@') {
        return Err(invalid_url());
    }
    let url = url::Url::parse(url).map_err(|_| invalid_url())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
        return Err(invalid_url());
    }
    platform::open(url.as_str())
}

fn invalid_url() -> io::Error { io::Error::new(io::ErrorKind::InvalidInput, "browser target must be an HTTP(S) URL without credentials or control characters") }

#[cfg(windows)]
mod platform {
    use super::io;
    use std::{ffi::c_void, ptr::{null, null_mut}};

    /// Win32 SHELLEXECUTEINFOW, including the icon/monitor union's pointer-sized slot.
    #[repr(C)]
    struct ShellExecuteInfo {
        cb_size: u32, f_mask: u32, hwnd: *mut c_void, verb: *const u16, file: *const u16,
        parameters: *const u16, directory: *const u16, show: i32, inst_app: *mut c_void,
        id_list: *mut c_void, class: *const u16, hkey_class: *mut c_void, hot_key: u32,
        icon_or_monitor: *mut c_void, process: *mut c_void,
    }
    #[cfg(not(test))]
    #[link(name = "shell32")]
    extern "system" { fn ShellExecuteExW(info: *mut ShellExecuteInfo) -> i32; }

    pub(super) fn open(url: &str) -> io::Result<()> {
        let target: Vec<u16> = url.encode_utf16().chain(Some(0)).collect();
        let verb: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
        let mut info = ShellExecuteInfo {
            cb_size: std::mem::size_of::<ShellExecuteInfo>() as u32,
            // SEE_MASK_FLAG_NO_UI | SEE_MASK_NOASYNC: report errors to the caller and
            // finish handing off the URL before this function's buffers/thread disappear.
            f_mask: 0x400 | 0x100, hwnd: null_mut(), verb: verb.as_ptr(), file: target.as_ptr(),
            parameters: null(), directory: null(), show: 1, inst_app: null_mut(), id_list: null_mut(),
            class: null(), hkey_class: null_mut(), hot_key: 0, icon_or_monitor: null_mut(), process: null_mut(),
        };
        // SAFETY: the structure matches SHELLEXECUTEINFOW and its NUL-terminated
        // UTF-16 strings outlive the call. The URL is lpFile, never a command line.
        if unsafe { ShellExecuteExW(&mut info) } != 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
    }

    // Intercept only the external API in tests. The public open() validation and actual
    // Win32 structure construction both run, while no browser or shell is ever started.
    #[cfg(test)]
    #[allow(non_snake_case)]
    unsafe fn ShellExecuteExW(info: *mut ShellExecuteInfo) -> i32 {
        let info = &*info;
        assert_eq!(info.cb_size as usize, std::mem::size_of::<ShellExecuteInfo>());
        assert_eq!(info.f_mask & 0x500, 0x500);
        assert!(info.parameters.is_null() && info.directory.is_null());
        unsafe fn read_wide(value: *const u16) -> String {
            let mut length = 0;
            while *value.add(length) != 0 { length += 1; }
            String::from_utf16(std::slice::from_raw_parts(value, length)).unwrap()
        }
        assert_eq!(read_wide(info.verb), "open");
        super::tests::LAUNCHES.with(|launches| launches.borrow_mut().push(read_wide(info.file)));
        1
    }
}

#[cfg(not(windows))]
mod platform {
    use super::io;
    pub(super) fn open(url: &str) -> io::Result<()> {
        let program = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        let status = std::process::Command::new(program).arg(url).status()?;
        if status.success() { Ok(()) } else { Err(io::Error::new(io::ErrorKind::Other, "browser launch failed")) }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::cell::RefCell;
    thread_local! { pub(super) static LAUNCHES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) }; }
    #[test]
    fn browser_receives_http_urls_as_one_data_target() {
        for (input, expected) in [
            ("HTTPS://Example.COM:443/", "https://example.com/"),
            ("http://localhost:8787/", "http://localhost:8787/"),
            ("https://example.com/path?a=1&b=2|3&quoted=\"x\"&env=%25PATH%25", "https://example.com/path?a=1&b=2|3&quoted=%22x%22&env=%25PATH%25"),
        ] {
            LAUNCHES.with(|launches| launches.borrow_mut().clear());
            open(input).unwrap();
            assert_eq!(LAUNCHES.with(|launches| launches.borrow().clone()), vec![expected.to_string()]);
        }
    }
    #[test]
    fn invalid_browser_targets_never_reach_the_os_launcher() {
        for input in ["file:///C:/Windows/System32/calc.exe", "javascript:alert(1)", "cmd.exe /C calc", "https:///example.com", "https:example.com", "https://user:password@example.com", "https://@example.com", "https://exam\nple.com", "https://example.com\0", "https://example.com\r", "https://example.com\u{7f}"] {
            LAUNCHES.with(|launches| launches.borrow_mut().clear());
            assert_eq!(open(input).unwrap_err().kind(), io::ErrorKind::InvalidInput, "{input:?}");
            assert!(LAUNCHES.with(|launches| launches.borrow().is_empty()), "{input:?}");
        }
    }
}
