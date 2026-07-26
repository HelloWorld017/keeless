//! Linux virtual HID device.
//!
//! Creates a FIDO-class HID device through `/dev/uhid` so browsers pick it up as
//! an ordinary security key: the kernel exposes it under `/dev/hidraw*`, and
//! systemd's FIDO rules grant the logged-in user access to it.
//!
//! Structure layouts follow `include/uapi/linux/uhid.h`. Events are read and
//! written as byte buffers in native order rather than transmuted structs, since
//! every field of the packed layout would otherwise be an unaligned access.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;

use tokio::io::unix::AsyncFd;

/// Event types, in the order `enum uhid_event_type` declares them.
mod event {
    pub const DESTROY: u32 = 1;
    pub const START: u32 = 2;
    pub const STOP: u32 = 3;
    pub const OPEN: u32 = 4;
    pub const CLOSE: u32 = 5;
    pub const OUTPUT: u32 = 6;
    pub const GET_REPORT: u32 = 9;
    pub const GET_REPORT_REPLY: u32 = 10;
    pub const CREATE2: u32 = 11;
    pub const INPUT2: u32 = 12;
    pub const SET_REPORT: u32 = 13;
    pub const SET_REPORT_REPLY: u32 = 14;
}

/// `UHID_DATA_MAX`, the payload capacity shared by every variable-length event.
const DATA_MAX: usize = 4096;

/// Size of `struct uhid_event`: the type tag plus the largest request, `create2`.
const EVENT_SIZE: usize = 4 + CREATE2_SIZE;
const CREATE2_SIZE: usize = 128 + 64 + 64 + 2 + 2 + 4 + 4 + 4 + 4 + DATA_MAX;

/// A FIDO HID report is always 64 bytes in each direction.
pub const REPORT_SIZE: usize = 64;

const BUS_USB: u16 = 0x03;

/// Report descriptor for a FIDO authenticator: usage page 0xF1D0, usage 0x01,
/// with one 64-byte input and one 64-byte output report.
const FIDO_REPORT_DESCRIPTOR: &[u8] = &[
    0x06, 0xd0, 0xf1, // Usage Page (FIDO Alliance)
    0x09, 0x01, // Usage (CTAPHID)
    0xa1, 0x01, // Collection (Application)
    0x09, 0x20, //   Usage (Input Report Data)
    0x15, 0x00, //   Logical Minimum (0)
    0x26, 0xff, 0x00, //   Logical Maximum (255)
    0x75, 0x08, //   Report Size (8)
    0x95, 0x40, //   Report Count (64)
    0x81, 0x02, //   Input (Data, Var, Abs)
    0x09, 0x21, //   Usage (Output Report Data)
    0x15, 0x00, //   Logical Minimum (0)
    0x26, 0xff, 0x00, //   Logical Maximum (255)
    0x75, 0x08, //   Report Size (8)
    0x95, 0x40, //   Report Count (64)
    0x91, 0x02, //   Output (Data, Var, Abs)
    0xc0, // End Collection
];

/// USB identifiers the virtual device reports.
///
/// Deliberately not a real vendor's: browsers special-case some vendors into
/// legacy U2F behaviour, which would hide the CTAP2 features this authenticator
/// depends on.
const VENDOR_ID: u32 = 0x1209; // pid.codes, the community vendor ID space
const PRODUCT_ID: u32 = 0x5031;
const VERSION: u32 = 0x0001;

/// A virtual HID device that stays alive as long as this value does.
pub struct UhidDevice {
    file: AsyncFd<File>,
    started: bool,
}

impl UhidDevice {
    /// Create the device and wait for the kernel to bring it up.
    pub async fn create(name: &str) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open("/dev/uhid")
            .map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!(
                        "cannot open /dev/uhid ({error}); run `keeless-vhid setup` to grant access"
                    ),
                )
            })?;
        let mut device = Self {
            file: AsyncFd::new(file)?,
            started: false,
        };
        device.write_event(&create2_event(name)).await?;

        // The kernel answers with START once the HID core has bound the device,
        // and only then will it deliver reports.
        while !device.started {
            match device.read_event().await? {
                Some(Event::Started) => device.started = true,
                Some(Event::Stopped) => {
                    return Err(io::Error::other(
                        "virtual HID device stopped before starting",
                    ));
                }
                _ => {}
            }
        }
        Ok(device)
    }

    /// Wait for the next report the host sent to this device.
    ///
    /// Report requests the kernel expects an answer to are handled here, because
    /// leaving one unanswered blocks the process that issued it.
    pub async fn read_report(&self) -> io::Result<Vec<u8>> {
        loop {
            match self.read_event().await? {
                Some(Event::Output(report)) => return Ok(report),
                Some(Event::GetReport { id }) => {
                    // A FIDO device carries no feature reports, so refuse politely.
                    self.write_event(&get_report_reply_event(id)).await?;
                }
                Some(Event::SetReport { id }) => {
                    self.write_event(&set_report_reply_event(id)).await?;
                }
                Some(Event::Stopped) => {
                    return Err(io::Error::other("virtual HID device was stopped"));
                }
                _ => {}
            }
        }
    }

    /// Send one report to the host.
    pub async fn write_report(&self, report: &[u8; REPORT_SIZE]) -> io::Result<()> {
        self.write_event(&input2_event(report)).await
    }

    async fn read_event(&self) -> io::Result<Option<Event>> {
        let mut buffer = vec![0_u8; EVENT_SIZE];
        loop {
            let mut guard = self.file.readable().await?;
            match guard.try_io(|inner| (&mut inner.get_ref()).read(&mut buffer)) {
                Ok(Ok(read)) => return Ok(parse_event(&buffer[..read])),
                Ok(Err(error)) => return Err(error),
                Err(_would_block) => continue,
            }
        }
    }

    async fn write_event(&self, event: &[u8]) -> io::Result<()> {
        loop {
            let mut guard = self.file.writable().await?;
            match guard.try_io(|inner| (&mut inner.get_ref()).write_all(event)) {
                Ok(result) => return result,
                Err(_would_block) => continue,
            }
        }
    }
}

impl Drop for UhidDevice {
    fn drop(&mut self) {
        // Best effort: the kernel also destroys the device when the fd closes.
        let _ = (&mut self.file.get_ref()).write_all(&destroy_event());
    }
}

enum Event {
    Started,
    Stopped,
    Output(Vec<u8>),
    GetReport { id: u32 },
    SetReport { id: u32 },
}

fn parse_event(bytes: &[u8]) -> Option<Event> {
    let kind = u32::from_ne_bytes(bytes.get(..4)?.try_into().ok()?);
    match kind {
        event::START | event::OPEN => Some(Event::Started),
        event::STOP | event::CLOSE => Some(Event::Stopped),
        event::OUTPUT => {
            // struct uhid_output_req { u8 data[4096]; u16 size; u8 rtype; }
            let size = u16::from_ne_bytes(bytes.get(4 + DATA_MAX..6 + DATA_MAX)?.try_into().ok()?);
            let data = bytes.get(4..4 + usize::from(size))?;
            // The HID core prefixes a report ID of zero for numbered-free devices.
            let report = match data.split_first() {
                Some((0, rest)) if rest.len() == REPORT_SIZE => rest,
                _ if data.len() == REPORT_SIZE => data,
                _ => return None,
            };
            Some(Event::Output(report.to_vec()))
        }
        event::GET_REPORT => Some(Event::GetReport {
            id: u32::from_ne_bytes(bytes.get(4..8)?.try_into().ok()?),
        }),
        event::SET_REPORT => Some(Event::SetReport {
            id: u32::from_ne_bytes(bytes.get(4..8)?.try_into().ok()?),
        }),
        _ => None,
    }
}

fn create2_event(name: &str) -> Vec<u8> {
    let mut event = vec![0_u8; 4 + CREATE2_SIZE];
    event[..4].copy_from_slice(&event::CREATE2.to_ne_bytes());
    write_cstr(&mut event[4..132], name);
    write_cstr(&mut event[132..196], "keeless");
    write_cstr(&mut event[196..260], "keeless-vhid");
    event[260..262].copy_from_slice(&(FIDO_REPORT_DESCRIPTOR.len() as u16).to_ne_bytes());
    event[262..264].copy_from_slice(&BUS_USB.to_ne_bytes());
    event[264..268].copy_from_slice(&VENDOR_ID.to_ne_bytes());
    event[268..272].copy_from_slice(&PRODUCT_ID.to_ne_bytes());
    event[272..276].copy_from_slice(&VERSION.to_ne_bytes());
    // Bytes 276..280 are the country code, which stays zero.
    event[280..280 + FIDO_REPORT_DESCRIPTOR.len()].copy_from_slice(FIDO_REPORT_DESCRIPTOR);
    event
}

fn input2_event(report: &[u8; REPORT_SIZE]) -> Vec<u8> {
    // struct uhid_input2_req { u16 size; u8 data[4096]; }
    let mut event = Vec::with_capacity(6 + REPORT_SIZE);
    event.extend_from_slice(&event::INPUT2.to_ne_bytes());
    event.extend_from_slice(&(REPORT_SIZE as u16).to_ne_bytes());
    event.extend_from_slice(report);
    event
}

fn destroy_event() -> Vec<u8> {
    event::DESTROY.to_ne_bytes().to_vec()
}

fn get_report_reply_event(id: u32) -> Vec<u8> {
    // struct uhid_get_report_reply_req { u32 id; u16 err; u16 size; u8 data[4096]; }
    let mut event = Vec::with_capacity(12);
    event.extend_from_slice(&event::GET_REPORT_REPLY.to_ne_bytes());
    event.extend_from_slice(&id.to_ne_bytes());
    event.extend_from_slice(&(libc::EIO as u16).to_ne_bytes());
    event.extend_from_slice(&0_u16.to_ne_bytes());
    event
}

fn set_report_reply_event(id: u32) -> Vec<u8> {
    // struct uhid_set_report_reply_req { u32 id; u16 err; }
    let mut event = Vec::with_capacity(10);
    event.extend_from_slice(&event::SET_REPORT_REPLY.to_ne_bytes());
    event.extend_from_slice(&id.to_ne_bytes());
    event.extend_from_slice(&(libc::EIO as u16).to_ne_bytes());
    event
}

/// Copy a name into a fixed-size, NUL-terminated field, truncating on a char boundary.
///
/// The remainder is zeroed rather than left as-is, so the field is terminated
/// whatever the buffer held before and no stale bytes reach the kernel.
fn write_cstr(field: &mut [u8], value: &str) {
    let limit = field.len() - 1;
    let mut end = value.len().min(limit);
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    field[..end].copy_from_slice(&value.as_bytes()[..end]);
    field[end..].fill(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_create_event_the_kernel_layout_expects() {
        let event = create2_event("Keeless");
        assert_eq!(event.len(), 4 + CREATE2_SIZE);
        assert_eq!(&event[..4], event::CREATE2.to_ne_bytes());
        assert_eq!(&event[4..11], b"Keeless");
        assert_eq!(event[11], 0, "the name field is NUL-terminated");
        assert_eq!(
            u16::from_ne_bytes(event[260..262].try_into().unwrap()) as usize,
            FIDO_REPORT_DESCRIPTOR.len()
        );
        assert_eq!(
            u16::from_ne_bytes(event[262..264].try_into().unwrap()),
            BUS_USB
        );
        assert_eq!(
            u32::from_ne_bytes(event[264..268].try_into().unwrap()),
            VENDOR_ID
        );
        assert_eq!(
            &event[280..280 + FIDO_REPORT_DESCRIPTOR.len()],
            FIDO_REPORT_DESCRIPTOR
        );
    }

    #[test]
    fn truncates_an_overlong_name_without_splitting_a_character() {
        let mut field = [0xff_u8; 8];
        write_cstr(&mut field, "가나다라마");
        assert_eq!(&field[..6], "가나".as_bytes());
        assert_eq!(&field[6..], [0, 0], "the field is terminated and cleared");

        // A short name clears whatever the field held before it.
        let mut field = [0xff_u8; 8];
        write_cstr(&mut field, "ab");
        assert_eq!(field, [b'a', b'b', 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn reads_output_reports_with_and_without_a_report_id() {
        let mut event = vec![0_u8; EVENT_SIZE];
        event[..4].copy_from_slice(&event::OUTPUT.to_ne_bytes());
        event[4] = 0x00;
        event[5..5 + REPORT_SIZE].copy_from_slice(&[0xab; REPORT_SIZE]);
        event[4 + DATA_MAX..6 + DATA_MAX]
            .copy_from_slice(&((REPORT_SIZE + 1) as u16).to_ne_bytes());
        let Some(Event::Output(report)) = parse_event(&event) else {
            panic!("expected an output report");
        };
        assert_eq!(report, vec![0xab; REPORT_SIZE]);

        let mut event = vec![0_u8; EVENT_SIZE];
        event[..4].copy_from_slice(&event::OUTPUT.to_ne_bytes());
        event[4..4 + REPORT_SIZE].copy_from_slice(&[0xcd; REPORT_SIZE]);
        event[4 + DATA_MAX..6 + DATA_MAX].copy_from_slice(&(REPORT_SIZE as u16).to_ne_bytes());
        let Some(Event::Output(report)) = parse_event(&event) else {
            panic!("expected an output report");
        };
        assert_eq!(report, vec![0xcd; REPORT_SIZE]);
    }

    #[test]
    fn ignores_truncated_and_unknown_events() {
        assert!(parse_event(&[]).is_none());
        assert!(parse_event(&0xdead_u32.to_ne_bytes()).is_none());

        // An OUTPUT event claiming more data than the buffer holds.
        let mut event = vec![0_u8; EVENT_SIZE];
        event[..4].copy_from_slice(&event::OUTPUT.to_ne_bytes());
        event[4 + DATA_MAX..6 + DATA_MAX].copy_from_slice(&99_u16.to_ne_bytes());
        assert!(parse_event(&event).is_none());
    }

    #[test]
    fn answers_report_requests_with_an_error() {
        let reply = get_report_reply_event(7);
        assert_eq!(&reply[..4], event::GET_REPORT_REPLY.to_ne_bytes());
        assert_eq!(u32::from_ne_bytes(reply[4..8].try_into().unwrap()), 7);
        assert_ne!(u16::from_ne_bytes(reply[8..10].try_into().unwrap()), 0);

        let reply = set_report_reply_event(9);
        assert_eq!(&reply[..4], event::SET_REPORT_REPLY.to_ne_bytes());
        assert_eq!(u32::from_ne_bytes(reply[4..8].try_into().unwrap()), 9);
        assert_ne!(u16::from_ne_bytes(reply[8..10].try_into().unwrap()), 0);
    }
}
