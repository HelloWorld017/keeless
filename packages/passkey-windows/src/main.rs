#![cfg_attr(windows, windows_subsystem = "windows")]
#[cfg(windows)]
fn main() {
    if let Err(error) = keeless_passkey_windows::main(std::env::args_os().skip(1)) {
        eprintln!("keeless-passkey-windows: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("keeless-passkey-windows only runs on Windows");
    std::process::exit(1);
}
