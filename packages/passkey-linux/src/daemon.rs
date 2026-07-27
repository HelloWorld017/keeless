//! The CTAPHID event loop driving the virtual HID device.

use std::time::{Duration, Instant};

use keeless_passkey_ctap::CtapStatus;
use keeless_passkey_ctap::response;
use tokio::sync::{mpsc, oneshot};

use crate::authenticator::{Authenticator, Progress};
use crate::ctaphid::{
    self, Accepted, Assembler, Command, MAX_PAYLOAD_SIZE, Message, TransportError,
};
use crate::uhid::UhidDevice;

/// Name the device reports, which is what browsers show in their key picker.
const DEVICE_NAME: &str = "Keeless";

/// How often to tell the host a running command is still alive.
///
/// The CTAPHID spec puts the ceiling at 100 ms; going slower makes clients treat
/// the authenticator as unresponsive and give up.
const KEEPALIVE_INTERVAL: Duration = Duration::from_millis(100);

/// Device version reported in an INIT response.
const DEVICE_VERSION: [u8; 3] = [0, 1, 0];

/// Run the authenticator until the device fails or `shutdown` resolves.
pub async fn run(
    authenticator: Authenticator,
    shutdown: impl std::future::Future<Output = ()>,
) -> std::io::Result<()> {
    let device = UhidDevice::create(DEVICE_NAME).await?;
    let mut state = State::Idle(Box::new(authenticator));
    let mut assembler = Assembler::new();
    let (completions, mut completed) = mpsc::channel::<Completion>(1);
    let mut keepalive = tokio::time::interval(KEEPALIVE_INTERVAL);
    keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut shutdown = std::pin::pin!(shutdown);

    loop {
        tokio::select! {
            () = &mut shutdown => return Ok(()),

            report = device.read_report() => {
                match assembler.accept(&report?, Instant::now()) {
                    Accepted::Incomplete => {}
                    Accepted::Message(message) => {
                        handle_message(&device, &mut assembler, &mut state, &completions, message)
                            .await?;
                    }
                    Accepted::Error(channel, error) => {
                        device.write_report(&ctaphid::encode_error(channel, error)).await?;
                    }
                }
            }

            Some(completion) = completed.recv() => {
                let answered = match &state {
                    State::Busy(command) => command.answered,
                    // Only a running command can produce a completion.
                    State::Idle(_) => true,
                };
                let channel = match &state {
                    State::Busy(command) => command.channel,
                    State::Idle(_) => 0,
                };
                state = State::Idle(completion.authenticator);
                if let Some(payload) = completion.payload.filter(|_| !answered) {
                    for report in ctaphid::encode(channel, Command::Cbor, &payload) {
                        device.write_report(&report).await?;
                    }
                }
            }

            _ = keepalive.tick(), if state.is_busy() => {
                if let State::Busy(command) = &state
                    && !command.answered
                {
                    let report =
                        ctaphid::encode_keepalive(command.channel, command.progress.status());
                    device.write_report(&report).await?;
                }
            }
        }
    }
}

/// Whether the authenticator is free, or lent to a running command.
enum State {
    Idle(Box<Authenticator>),
    Busy(RunningCommand),
}

impl State {
    fn is_busy(&self) -> bool {
        matches!(self, Self::Busy(_))
    }
}

struct RunningCommand {
    channel: u32,
    progress: Progress,
    /// Dropped to cancel; the command answers by returning the authenticator.
    cancel: Option<oneshot::Sender<()>>,
    /// Set once the channel has been answered, which CANCEL does immediately.
    answered: bool,
}

/// A finished command handing the authenticator back.
struct Completion {
    authenticator: Box<Authenticator>,
    /// Absent when the command was cancelled before producing a response.
    payload: Option<Vec<u8>>,
}

async fn handle_message(
    device: &UhidDevice,
    assembler: &mut Assembler,
    state: &mut State,
    completions: &mpsc::Sender<Completion>,
    message: Message,
) -> std::io::Result<()> {
    match message.command {
        Command::Init => {
            let allocated = if message.channel == ctaphid::BROADCAST_CHANNEL {
                assembler.allocate_channel()
            } else {
                // A client re-initializing its own channel keeps it and asks for
                // everything pending on it to be abandoned. That is a client's
                // only way to recover from a request it can no longer answer for,
                // so it has to reach the running command, not just the reassembly
                // buffer.
                assembler.cancel(message.channel);
                abandon(state, message.channel);
                message.channel
            };
            let report =
                ctaphid::encode_init(message.channel, &message.payload, allocated, DEVICE_VERSION);
            device.write_report(&report).await
        }

        Command::Ping => {
            for report in ctaphid::encode(message.channel, Command::Ping, &message.payload) {
                device.write_report(&report).await?;
            }
            Ok(())
        }

        Command::Cancel => {
            assembler.cancel(message.channel);
            if !abandon(state, message.channel) {
                return Ok(());
            }
            let payload = response::status(CtapStatus::KeepaliveCancel);
            for report in ctaphid::encode(message.channel, Command::Cbor, &payload) {
                device.write_report(&report).await?;
            }
            Ok(())
        }

        Command::Cbor => {
            let authenticator = match std::mem::replace(state, State::Busy(placeholder())) {
                State::Idle(authenticator) => authenticator,
                busy => {
                    *state = busy;
                    let report =
                        ctaphid::encode_error(message.channel, TransportError::ChannelBusy);
                    return device.write_report(&report).await;
                }
            };
            let progress = Progress::default();
            let (cancel, cancelled) = oneshot::channel();
            spawn_command(
                authenticator,
                completions.clone(),
                message.payload,
                progress.clone(),
                cancelled,
            );
            *state = State::Busy(RunningCommand {
                channel: message.channel,
                progress,
                cancel: Some(cancel),
                answered: false,
            });
            Ok(())
        }

        // Wink has no meaning for a device with nothing to blink, and MSG is the
        // legacy U2F framing this authenticator does not advertise.
        Command::Msg
        | Command::Wink
        | Command::Lock
        | Command::Keepalive
        | Command::Error
        | Command::Unknown(_) => {
            let report = ctaphid::encode_error(message.channel, TransportError::InvalidCommand);
            device.write_report(&report).await
        }
    }
}

/// Run one CBOR command without blocking the transport loop.
///
/// A command can wait minutes on the user, and INIT, PING and CANCEL must keep
/// working meanwhile — CANCEL especially, since it is how a browser withdraws a
/// request the user is still looking at.
fn spawn_command(
    mut authenticator: Box<Authenticator>,
    completions: mpsc::Sender<Completion>,
    payload: Vec<u8>,
    progress: Progress,
    cancelled: oneshot::Receiver<()>,
) {
    tokio::spawn(async move {
        let payload = tokio::select! {
            response = authenticator.handle(&payload, &progress) => Some(response),
            _ = cancelled => None,
        };
        let payload = payload.map(|response| {
            if response.len() > MAX_PAYLOAD_SIZE {
                response::status(CtapStatus::RequestTooLarge)
            } else {
                response
            }
        });
        // The loop always keeps the receiver alive while a command runs, so a
        // send failure only happens during shutdown.
        let _ = completions
            .send(Completion {
                authenticator,
                payload,
            })
            .await;
    });
}

/// Abandon the command running on `channel`, if any.
///
/// Returns whether a command was abandoned, which is also whether the channel
/// still owes an answer. Dropping the cancel sender drops the command's future,
/// taking any consent prompt down with it; the authenticator comes back through
/// the completion channel, and its response is discarded.
fn abandon(state: &mut State, channel: u32) -> bool {
    let State::Busy(command) = state else {
        return false;
    };
    if command.channel != channel || command.answered {
        return false;
    }
    command.cancel = None;
    command.answered = true;
    true
}

/// Stand-in used only while the authenticator is moved out of `State`.
fn placeholder() -> RunningCommand {
    RunningCommand {
        channel: 0,
        progress: Progress::default(),
        cancel: None,
        answered: true,
    }
}
