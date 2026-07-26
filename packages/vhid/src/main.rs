fn main() {
    if let Err(error) = keeless_vhid::main(std::env::args_os().skip(1)) {
        eprintln!("keeless-vhid: {error}");
        std::process::exit(error.exit_code());
    }
}
