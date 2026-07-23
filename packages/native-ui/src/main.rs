fn main() {
    if let Err(error) = keeless_native_ui::run(std::env::args_os().skip(1)) {
        eprintln!("keeless-native-ui: {error}");
        std::process::exit(error.exit_code());
    }
}
