fn main() {
    if let Err(error) = keeless_passkey_linux::main(std::env::args_os().skip(1)) {
        eprintln!("keeless-passkey-linux: {error}");
        std::process::exit(error.exit_code());
    }
}
