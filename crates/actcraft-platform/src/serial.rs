//! Serial ports, for USB DMX: which ports the computer has, and one opened for DMX. A USB DMX
//! device shows up as a serial port (`COM3` on Windows, `/dev/ttyUSB0` or `/dev/cu.usbserial-…`
//! elsewhere). Two kinds are driven:
//!
//! - an interface with its own processor that speaks the DMX USB Pro serial protocol: messages
//!   go to it as they are, and it makes the DMX signal;
//! - a plain USB-to-RS-485 cable (an FTDI chip and a line driver): the computer makes the DMX
//!   signal itself, frame by frame — a break, the mark after it, then the start code and the 512
//!   levels at 250 000 baud, 8 data bits, no parity, two stop bits.

use std::io;
use std::time::Duration;

use serial2::{CharSize, FlowControl, Parity, Settings, StopBits};

/// What is on the other end of a port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    /// An interface with its own processor (DMX USB Pro serial protocol).
    Interface,
    /// A plain USB-to-RS-485 cable; the computer times the DMX signal.
    Cable,
}

/// DMX's bit rate.
const DMX_BAUD: u32 = 250_000;
/// Longer than DMX's shortest break (88 µs) and mark after break (12 µs); the system's sleep
/// may make them longer, which DMX allows.
const BREAK: Duration = Duration::from_micros(150);
const MARK_AFTER_BREAK: Duration = Duration::from_micros(20);
/// The time a whole frame takes on the line (513 slots of 11 bits at 250 000 baud, about 22.6
/// ms) and a little more: the next break must not cut into it.
const FRAME_ON_LINE: Duration = Duration::from_micros(23_500);

/// An open serial port.
pub struct SerialPort {
    port: serial2::SerialPort,
}

impl SerialPort {
    /// Opens the port `name` for what is on its other end.
    pub fn open(name: &str, line: Line) -> io::Result<Self> {
        let port = serial2::SerialPort::open(name, move |mut s: Settings| {
            s.set_raw();
            match line {
                // The interface ignores the rate (it is USB), but the port wants one.
                Line::Interface => s.set_baud_rate(57_600)?,
                Line::Cable => {
                    s.set_baud_rate(DMX_BAUD)?;
                    s.set_char_size(CharSize::Bits8);
                    s.set_stop_bits(StopBits::Two);
                    s.set_parity(Parity::None);
                }
            }
            s.set_flow_control(FlowControl::None);
            Ok(s)
        })?;
        Ok(Self { port })
    }

    /// Writes every byte and waits until the port has them.
    pub fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.port.write_all(bytes)?;
        self.port.flush()
    }

    /// Sends one DMX frame down a plain cable: break, mark after break, start code 0 and the
    /// levels; returns once the frame is on the line.
    pub fn send_frame(&mut self, levels: &[u8; 512]) -> io::Result<()> {
        self.port.set_break(true)?;
        std::thread::sleep(BREAK);
        self.port.set_break(false)?;
        std::thread::sleep(MARK_AFTER_BREAK);
        let mut frame = [0u8; 513];
        frame[1..].copy_from_slice(levels);
        self.write_all(&frame)?;
        std::thread::sleep(FRAME_ON_LINE);
        Ok(())
    }
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
        assert!(SerialPort::open("ActCraftNoSuchPort9", Line::Interface).is_err());
        assert!(SerialPort::open("ActCraftNoSuchPort9", Line::Cable).is_err());
        let names = ports();
        assert!(names.windows(2).all(|w| w[0] < w[1]), "sorted, once each: {names:?}");
    }

    #[test]
    fn a_frame_on_a_cable_takes_its_time_on_the_line() {
        // 513 slots of 11 bits at 250 000 baud.
        let on_line = Duration::from_micros(513 * 11 * 1_000_000 / u64::from(DMX_BAUD));
        assert!(FRAME_ON_LINE > on_line, "{on_line:?}");
    }
}
