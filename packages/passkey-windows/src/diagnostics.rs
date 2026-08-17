#[cfg(debug_assertions)]
use std::{
    fmt,
    fs::{File, OpenOptions},
    io::Write,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(debug_assertions)]
use keeless_host_desktop_shared::FileStore;

#[cfg(debug_assertions)]
use crate::session::STATE_FILE;

#[cfg(debug_assertions)]
static LOG: OnceLock<Mutex<File>> = OnceLock::new();

#[cfg(debug_assertions)]
pub(crate) fn initialize() {
    let Ok(store) = FileStore::project(STATE_FILE) else {
        return;
    };
    let Ok(timestamp) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return;
    };
    let path = store
        .directory()
        .map(|directory| directory.join(format!("passkey_{}.log", timestamp.as_nanos())));
    let Ok(path) = path else {
        return;
    };
    let Ok(mut file) = OpenOptions::new().append(true).create_new(true).open(path) else {
        return;
    };
    let _ = writeln!(file, "keeless-passkey-windows diagnostic log");
    let _ = file.flush();
    let _ = LOG.set(Mutex::new(file));
}

#[cfg(not(debug_assertions))]
pub(crate) fn initialize() {}

#[cfg(debug_assertions)]
pub(crate) fn log(message: fmt::Arguments<'_>) {
    let Some(log) = LOG.get() else {
        return;
    };
    let Ok(mut log) = log.lock() else {
        return;
    };
    let _ = writeln!(log, "{message}");
    let _ = log.flush();
}

#[cfg(not(debug_assertions))]
pub(crate) fn log(_: std::fmt::Arguments<'_>) {}

macro_rules! diagnostic {
    ($($arg:tt)*) => {
        $crate::diagnostics::log(format_args!($($arg)*))
    };
}

pub(crate) use diagnostic;
