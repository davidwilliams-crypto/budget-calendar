//! Minimal replacement for the `webbrowser` crate (see Cargo.toml). Opens a URL with the OS default handler.
use std::io;
use std::process::Command;

pub fn open(url: &str) -> io::Result<()> {
    let status = if cfg!(target_os = "windows") {
        Command::new("cmd").args(["/C", "start", "", url]).status()?
    } else if cfg!(target_os = "macos") {
        Command::new("open").arg(url).status()?
    } else {
        Command::new("xdg-open").arg(url).status()?
    };
    if status.success() { Ok(()) } else { Err(io::Error::new(io::ErrorKind::Other, "browser launch failed")) }
}
