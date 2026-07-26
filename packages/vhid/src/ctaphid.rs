//! CTAPHID framing: 64-byte reports carrying fragmented messages over channels.

use std::collections::HashMap;
use std::time::{Duration, Instant};

pub const REPORT_SIZE: usize = 64;
pub const BROADCAST_CHANNEL: u32 = 0xffff_ffff;

/// Bytes a message can carry: one initialization packet plus 128 continuations.
pub const MAX_PAYLOAD_SIZE: usize = INIT_PAYLOAD_SIZE + CONT_PAYLOAD_SIZE * 128;

const INIT_PAYLOAD_SIZE: usize = REPORT_SIZE - 7;
const CONT_PAYLOAD_SIZE: usize = REPORT_SIZE - 5;
const INIT_MARKER: u8 = 0x80;

/// How long a partially received message may sit before the channel is reset.
const TRANSACTION_TIMEOUT: Duration = Duration::from_millis(3000);

/// Capabilities reported by INIT: CBOR commands, and no legacy U2F messages.
const CAPABILITY_CBOR: u8 = 0x04;
const CAPABILITY_NO_MSG: u8 = 0x08;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Ping,
    Msg,
    Lock,
    Init,
    Wink,
    Cbor,
    Cancel,
    Keepalive,
    Error,
    Unknown(u8),
}

impl Command {
    fn from_u8(value: u8) -> Self {
        match value {
            0x01 => Self::Ping,
            0x03 => Self::Msg,
            0x04 => Self::Lock,
            0x06 => Self::Init,
            0x08 => Self::Wink,
            0x10 => Self::Cbor,
            0x11 => Self::Cancel,
            0x3b => Self::Keepalive,
            0x3f => Self::Error,
            other => Self::Unknown(other),
        }
    }

    fn as_u8(self) -> u8 {
        match self {
            Self::Ping => 0x01,
            Self::Msg => 0x03,
            Self::Lock => 0x04,
            Self::Init => 0x06,
            Self::Wink => 0x08,
            Self::Cbor => 0x10,
            Self::Cancel => 0x11,
            Self::Keepalive => 0x3b,
            Self::Error => 0x3f,
            Self::Unknown(value) => value,
        }
    }
}

/// CTAPHID transport errors, distinct from the CTAP2 status codes in a payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TransportError {
    InvalidCommand = 0x01,
    InvalidLength = 0x03,
    InvalidSequence = 0x04,
    MessageTimeout = 0x05,
    ChannelBusy = 0x06,
    InvalidChannel = 0x0b,
    Other = 0x7f,
}

/// Progress reported while a command is still running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum KeepaliveStatus {
    Processing = 1,
    UserPresenceNeeded = 2,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Message {
    pub channel: u32,
    pub command: Command,
    pub payload: Vec<u8>,
}

/// Reassembles messages arriving as 64-byte reports, one partial per channel.
#[derive(Default)]
pub struct Assembler {
    partials: HashMap<u32, Partial>,
    next_channel: u32,
}

struct Partial {
    command: Command,
    expected: usize,
    payload: Vec<u8>,
    next_sequence: u8,
    started_at: Instant,
}

/// What the caller should do with a report.
pub enum Accepted {
    /// Still waiting for continuation packets.
    Incomplete,
    Message(Message),
    /// Answer the channel with a transport error.
    Error(u32, TransportError),
}

impl Assembler {
    pub fn new() -> Self {
        Self {
            partials: HashMap::new(),
            // Channel 0 is reserved and the broadcast channel is all ones.
            next_channel: 1,
        }
    }

    /// Take one report, returning a message once its last fragment arrives.
    pub fn accept(&mut self, report: &[u8], now: Instant) -> Accepted {
        self.expire(now);
        if report.len() != REPORT_SIZE {
            return Accepted::Error(0, TransportError::InvalidLength);
        }
        let channel = u32::from_be_bytes([report[0], report[1], report[2], report[3]]);
        if channel == 0 {
            return Accepted::Error(channel, TransportError::InvalidChannel);
        }

        if report[4] & INIT_MARKER == 0 {
            return self.accept_continuation(channel, report);
        }

        let command = Command::from_u8(report[4] & !INIT_MARKER);
        let expected = usize::from(u16::from_be_bytes([report[5], report[6]]));
        if expected > MAX_PAYLOAD_SIZE {
            self.partials.remove(&channel);
            return Accepted::Error(channel, TransportError::InvalidLength);
        }
        // An initialization packet always abandons any partial on its channel,
        // which is how the spec lets a client recover from a lost fragment.
        self.partials.remove(&channel);
        if command == Command::Cancel {
            return Accepted::Message(Message {
                channel,
                command,
                payload: Vec::new(),
            });
        }

        let payload = report[7..7 + expected.min(INIT_PAYLOAD_SIZE)].to_vec();
        if payload.len() == expected {
            return Accepted::Message(Message {
                channel,
                command,
                payload,
            });
        }
        self.partials.insert(
            channel,
            Partial {
                command,
                expected,
                payload,
                next_sequence: 0,
                started_at: now,
            },
        );
        Accepted::Incomplete
    }

    fn accept_continuation(&mut self, channel: u32, report: &[u8]) -> Accepted {
        let Some(partial) = self.partials.get_mut(&channel) else {
            // A continuation for a channel with nothing in progress is spurious.
            return Accepted::Error(channel, TransportError::InvalidSequence);
        };
        let sequence = report[4];
        if sequence != partial.next_sequence {
            self.partials.remove(&channel);
            return Accepted::Error(channel, TransportError::InvalidSequence);
        }
        partial.next_sequence += 1;

        let remaining = partial.expected - partial.payload.len();
        partial
            .payload
            .extend_from_slice(&report[5..5 + remaining.min(CONT_PAYLOAD_SIZE)]);
        if partial.payload.len() < partial.expected {
            return Accepted::Incomplete;
        }
        let partial = self.partials.remove(&channel).expect("partial exists");
        Accepted::Message(Message {
            channel,
            command: partial.command,
            payload: partial.payload,
        })
    }

    /// Allocate a channel for an INIT request.
    pub fn allocate_channel(&mut self) -> u32 {
        // Wrapping keeps allocation going after 2^32 channels, and skips the two
        // reserved values so a client can never be handed one.
        loop {
            let channel = self.next_channel;
            self.next_channel = self.next_channel.wrapping_add(1);
            if channel != 0 && channel != BROADCAST_CHANNEL {
                return channel;
            }
        }
    }

    /// Drop a channel's partial message, as CANCEL and a completed command do.
    pub fn cancel(&mut self, channel: u32) {
        self.partials.remove(&channel);
    }

    fn expire(&mut self, now: Instant) {
        self.partials
            .retain(|_, partial| now.duration_since(partial.started_at) < TRANSACTION_TIMEOUT);
    }
}

/// Split a message into the reports that carry it.
pub fn encode(channel: u32, command: Command, payload: &[u8]) -> Vec<[u8; REPORT_SIZE]> {
    let mut reports = Vec::new();
    let mut report = [0_u8; REPORT_SIZE];
    report[..4].copy_from_slice(&channel.to_be_bytes());
    report[4] = command.as_u8() | INIT_MARKER;
    report[5..7].copy_from_slice(&(payload.len() as u16).to_be_bytes());
    let first = payload.len().min(INIT_PAYLOAD_SIZE);
    report[7..7 + first].copy_from_slice(&payload[..first]);
    reports.push(report);

    for (index, chunk) in payload[first..].chunks(CONT_PAYLOAD_SIZE).enumerate() {
        let mut report = [0_u8; REPORT_SIZE];
        report[..4].copy_from_slice(&channel.to_be_bytes());
        report[4] = index as u8;
        report[5..5 + chunk.len()].copy_from_slice(chunk);
        reports.push(report);
    }
    reports
}

/// The single report answering an INIT request.
pub fn encode_init(
    channel: u32,
    nonce: &[u8],
    allocated: u32,
    version: [u8; 3],
) -> [u8; REPORT_SIZE] {
    let mut payload = Vec::with_capacity(17);
    payload.extend_from_slice(nonce);
    payload.resize(8, 0);
    payload.extend_from_slice(&allocated.to_be_bytes());
    payload.push(2); // CTAPHID protocol version
    payload.extend_from_slice(&version);
    payload.push(CAPABILITY_CBOR | CAPABILITY_NO_MSG);
    encode(channel, Command::Init, &payload)
        .pop()
        .expect("an INIT response fits one report")
}

pub fn encode_error(channel: u32, error: TransportError) -> [u8; REPORT_SIZE] {
    encode(channel, Command::Error, &[error as u8])
        .pop()
        .expect("an error fits one report")
}

pub fn encode_keepalive(channel: u32, status: KeepaliveStatus) -> [u8; REPORT_SIZE] {
    encode(channel, Command::Keepalive, &[status as u8])
        .pop()
        .expect("a keepalive fits one report")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assemble(assembler: &mut Assembler, reports: &[[u8; REPORT_SIZE]]) -> Option<Message> {
        let now = Instant::now();
        let mut message = None;
        for report in reports {
            match assembler.accept(report, now) {
                Accepted::Incomplete => {}
                Accepted::Message(value) => message = Some(value),
                Accepted::Error(channel, error) => {
                    panic!("unexpected error {error:?} on {channel}")
                }
            }
        }
        message
    }

    #[test]
    fn round_trips_a_message_that_spans_many_reports() {
        let payload: Vec<u8> = (0..1000).map(|value| value as u8).collect();
        let reports = encode(0x1234_5678, Command::Cbor, &payload);
        assert!(reports.len() > 1);
        assert_eq!(reports[0][4], 0x10 | INIT_MARKER);
        assert_eq!(reports[1][4], 0, "continuations start at sequence zero");

        let message = assemble(&mut Assembler::new(), &reports).expect("a complete message");
        assert_eq!(message.channel, 0x1234_5678);
        assert_eq!(message.command, Command::Cbor);
        assert_eq!(message.payload, payload);
    }

    #[test]
    fn round_trips_a_message_that_fits_one_report() {
        let reports = encode(7, Command::Ping, b"hello");
        assert_eq!(reports.len(), 1);
        let message = assemble(&mut Assembler::new(), &reports).expect("a complete message");
        assert_eq!(message.payload, b"hello");
    }

    #[test]
    fn round_trips_a_payload_that_exactly_fills_the_first_report() {
        let payload = vec![0xab; INIT_PAYLOAD_SIZE];
        let reports = encode(7, Command::Cbor, &payload);
        assert_eq!(reports.len(), 1);
        assert_eq!(
            assemble(&mut Assembler::new(), &reports).unwrap().payload,
            payload
        );
    }

    #[test]
    fn rejects_out_of_order_and_orphan_continuations() {
        let mut assembler = Assembler::new();
        let now = Instant::now();
        let reports = encode(9, Command::Cbor, &[1_u8; 200]);

        assert!(matches!(
            assembler.accept(&reports[1], now),
            Accepted::Error(9, TransportError::InvalidSequence)
        ));

        assert!(matches!(
            assembler.accept(&reports[0], now),
            Accepted::Incomplete
        ));
        let mut wrong = reports[1];
        wrong[4] = 5;
        assert!(matches!(
            assembler.accept(&wrong, now),
            Accepted::Error(9, TransportError::InvalidSequence)
        ));
        // The channel was reset, so the next continuation is an orphan too.
        assert!(matches!(
            assembler.accept(&reports[2], now),
            Accepted::Error(9, TransportError::InvalidSequence)
        ));
    }

    #[test]
    fn rejects_reserved_channels_and_impossible_lengths() {
        let mut assembler = Assembler::new();
        let now = Instant::now();

        let mut zero_channel = encode(0, Command::Ping, b"x")[0];
        zero_channel[..4].copy_from_slice(&0_u32.to_be_bytes());
        assert!(matches!(
            assembler.accept(&zero_channel, now),
            Accepted::Error(0, TransportError::InvalidChannel)
        ));

        let mut oversized = encode(4, Command::Cbor, b"x")[0];
        oversized[5..7].copy_from_slice(&u16::MAX.to_be_bytes());
        assert!(matches!(
            assembler.accept(&oversized, now),
            Accepted::Error(4, TransportError::InvalidLength)
        ));

        assert!(matches!(
            assembler.accept(&[0_u8; 8], now),
            Accepted::Error(0, TransportError::InvalidLength)
        ));
    }

    #[test]
    fn an_initialization_packet_abandons_a_partial_message() {
        let mut assembler = Assembler::new();
        let now = Instant::now();
        let long = encode(3, Command::Cbor, &vec![7_u8; 300]);
        assert!(matches!(
            assembler.accept(&long[0], now),
            Accepted::Incomplete
        ));

        let short = encode(3, Command::Ping, b"restart");
        let Accepted::Message(message) = assembler.accept(&short[0], now) else {
            panic!("expected the new message to complete");
        };
        assert_eq!(message.payload, b"restart");
        // The abandoned message's continuations are now orphans.
        assert!(matches!(
            assembler.accept(&long[1], now),
            Accepted::Error(3, TransportError::InvalidSequence)
        ));
    }

    #[test]
    fn drops_partial_messages_that_stall() {
        let mut assembler = Assembler::new();
        let started = Instant::now();
        let reports = encode(2, Command::Cbor, &vec![0_u8; 300]);
        assert!(matches!(
            assembler.accept(&reports[0], started),
            Accepted::Incomplete
        ));
        let later = started + TRANSACTION_TIMEOUT + Duration::from_millis(1);
        assert!(matches!(
            assembler.accept(&reports[1], later),
            Accepted::Error(2, TransportError::InvalidSequence)
        ));
    }

    #[test]
    fn cancel_arrives_as_a_message_even_mid_transaction() {
        let mut assembler = Assembler::new();
        let now = Instant::now();
        let reports = encode(5, Command::Cbor, &vec![0_u8; 300]);
        assert!(matches!(
            assembler.accept(&reports[0], now),
            Accepted::Incomplete
        ));

        let cancel = encode(5, Command::Cancel, &[]);
        let Accepted::Message(message) = assembler.accept(&cancel[0], now) else {
            panic!("expected a cancel message");
        };
        assert_eq!(message.command, Command::Cancel);
    }

    #[test]
    fn allocates_channels_skipping_the_reserved_values() {
        let mut assembler = Assembler::new();
        assert_eq!(assembler.allocate_channel(), 1);
        assert_eq!(assembler.allocate_channel(), 2);

        assembler.next_channel = BROADCAST_CHANNEL;
        assert_eq!(assembler.allocate_channel(), 1);
    }

    #[test]
    fn encodes_an_init_response_with_cbor_capabilities() {
        let nonce = [1, 2, 3, 4, 5, 6, 7, 8];
        let report = encode_init(BROADCAST_CHANNEL, &nonce, 0x0000_002a, [1, 2, 3]);
        assert_eq!(&report[..4], BROADCAST_CHANNEL.to_be_bytes());
        assert_eq!(report[4], 0x06 | INIT_MARKER);
        assert_eq!(u16::from_be_bytes([report[5], report[6]]), 17);
        assert_eq!(&report[7..15], nonce);
        assert_eq!(u32::from_be_bytes(report[15..19].try_into().unwrap()), 42);
        assert_eq!(report[19], 2);
        assert_eq!(&report[20..23], [1, 2, 3]);
        assert_eq!(report[23], CAPABILITY_CBOR | CAPABILITY_NO_MSG);
    }

    #[test]
    fn encodes_errors_and_keepalives() {
        let report = encode_error(6, TransportError::ChannelBusy);
        assert_eq!(report[4], 0x3f | INIT_MARKER);
        assert_eq!(report[7], 0x06);

        let report = encode_keepalive(6, KeepaliveStatus::UserPresenceNeeded);
        assert_eq!(report[4], 0x3b | INIT_MARKER);
        assert_eq!(report[7], 2);
    }
}
