fn main() {
    monitor_parent();
    if let Err(error) = keeless_native_ui::run(std::env::args_os().skip(1)) {
        eprintln!("keeless-native-ui: {error}");
        std::process::exit(error.exit_code());
    }
}

fn monitor_parent() {
    std::thread::spawn(|| {
        use std::io::Read as _;

        let mut input = std::io::stdin().lock();
        let mut buffer = [0_u8; 64];
        loop {
            match input.read(&mut buffer) {
                Ok(0) | Err(_) => std::process::exit(1),
                Ok(_) => {}
            }
        }
    });
}
