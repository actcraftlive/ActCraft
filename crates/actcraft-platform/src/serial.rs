//! Serial ports, for USB DMX interfaces: which ports the computer has, and one opened for raw
//! writing. A USB interface shows up as a serial port (`COM3` on Windows, `/dev/ttyUSB0` or
//! `/dev/cu.usbserial-…` elsewhere); the bytes go out exactly as given.

use std::fs::File;
use std::io::{self, Write};

/// An open serial port.
pub struct SerialPort {
    file: File,
}

impl SerialPort {
    /// Opens the port `name` for writing raw bytes.
    pub fn open(name: &str) -> io::Result<Self> {
        open_raw(name).map(|file| Self { file })
    }

    /// Writes every byte and waits until the port has them.
    pub fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.file.write_all(bytes)?;
        self.file.flush()
    }
}

#[cfg(windows)]
fn open_raw(name: &str) -> io::Result<File> {
    // COM10 and above open only with the device prefix; it works for every number.
    let path = if name.starts_with(r"\\.\") { name.to_owned() } else { format!(r"\\.\{name}") };
    std::fs::OpenOptions::new().write(true).open(path)
}

#[cfg(unix)]
fn open_raw(name: &str) -> io::Result<File> {
    use nix::fcntl::{FcntlArg, OFlag, fcntl};
    use nix::sys::termios::{self, BaudRate, ControlFlags, SetArg};
    use std::os::unix::fs::OpenOptionsExt;
    // Not as this program's terminal, and without waiting for a modem carrier while opening.
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags((OFlag::O_NOCTTY | OFlag::O_NONBLOCK).bits())
        .open(name)?;
    // Raw: no line ending translation or other changes to the bytes.
    let mut t = termios::tcgetattr(&file)?;
    termios::cfmakeraw(&mut t);
    t.control_flags |= ControlFlags::CLOCAL | ControlFlags::CREAD;
    // The interface ignores the rate (it is USB), but the port wants one.
    termios::cfsetspeed(&mut t, BaudRate::B57600)?;
    termios::tcsetattr(&file, SetArg::TCSANOW, &t)?;
    // Writes wait again from here on.
    fcntl(&file, FcntlArg::F_SETFL(OFlag::empty()))?;
    Ok(file)
}

#[cfg(not(any(unix, windows)))]
fn open_raw(_name: &str) -> io::Result<File> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "no serial ports on this system"))
}

/// The serial ports this computer has now, by name, sorted.
pub fn ports() -> Vec<String> {
    let mut names = list();
    names.sort();
    names.dedup();
    names
}

#[cfg(windows)]
fn list() -> Vec<String> {
    // Windows keeps the ports it has under this key: one value per port, its data the name.
    let Ok(key) = windows_registry::LOCAL_MACHINE.open(r"HARDWARE\DEVICEMAP\SERIALCOMM") else { return Vec::new() };
    let Ok(values) = key.values() else { return Vec::new() };
    values.filter_map(|(_, value)| String::try_from(value).ok()).filter(|n| !n.is_empty()).collect()
}

#[cfg(unix)]
fn list() -> Vec<String> {
    // USB serial adapters (FTDI and others) and USB modem-class devices; on macOS the callout
    // devices, which open without waiting for a carrier.
    const KINDS: &[&str] = &["ttyUSB", "ttyACM", "cu.usbserial", "cu.usbmodem"];
    let Ok(dir) = std::fs::read_dir("/dev") else { return Vec::new() };
    dir.filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| KINDS.iter().any(|k| name.starts_with(k)))
        .map(|name| format!("/dev/{name}"))
        .collect()
}

#[cfg(not(any(unix, windows)))]
fn list() -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_port_that_is_not_there_fails_to_open_and_listing_never_fails() {
        assert!(SerialPort::open("ActCraftNoSuchPort9").is_err());
        let names = ports();
        assert!(names.windows(2).all(|w| w[0] < w[1]), "sorted, once each: {names:?}");
    }
}
