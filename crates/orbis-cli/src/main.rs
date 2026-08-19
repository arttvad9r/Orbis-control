use std::process::ExitCode;

const HELP: &str = "orbisctl — Orbis Control command-line client\n\nUSAGE:\n    orbisctl [OPTIONS] <COMMAND>\n\nOPTIONS:\n    -h, --help       Print help\n    -V, --version    Print version\n\nCOMMANDS:\n    status            Read-only status (not implemented yet)\n";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);

    match (args.next().as_deref(), args.next()) {
        (None, _) | (Some("-h" | "--help"), None) => {
            print!("{HELP}");
            ExitCode::SUCCESS
        }
        (Some("-V" | "--version"), None) => {
            println!("orbisctl {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        (Some("status"), None) => {
            eprintln!("orbisctl: status is not implemented yet; Session1-backed read-only status is tracked in issue #119");
            ExitCode::from(2)
        }
        (Some(arg), _) => {
            eprintln!("orbisctl: unknown or invalid argument: {arg}");
            eprintln!("Try 'orbisctl --help'.");
            ExitCode::from(2)
        }
    }
}
