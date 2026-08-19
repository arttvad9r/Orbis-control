use std::fmt::Debug;
use std::process::ExitCode;

use orbis_providers::error::ProviderError;
use orbis_providers::traits::{
    BatteryProvider, GpuAccessProvider, GpuMuxProvider, GpuPowerProvider, PerformanceProvider,
};
use orbis_session_client::{
    SessionChargeLimitProvider, SessionGpuAccessProvider, SessionGpuMuxProvider,
    SessionGpuPowerProvider, SessionPerformanceProvider, ZbusSessionChargeLimitSource,
    ZbusSessionGpuSource, ZbusSessionPerformanceSource,
};

const HELP: &str = "orbisctl — Orbis Control read-only command-line client\n\nUSAGE:\n    orbisctl [OPTIONS] <COMMAND>\n\nOPTIONS:\n    -h, --help       Print help\n    -V, --version    Print version\n\nCOMMANDS:\n    status            Read current Session1 state without performing mutations\n";

#[derive(Debug, Clone, PartialEq, Eq)]
enum Command {
    Help,
    Version,
    Status,
    Invalid(String),
}

fn parse_args<I>(args: I) -> Command
where
    I: IntoIterator<Item = String>,
{
    let mut args = args.into_iter();
    let first = args.next();
    let extra = args.next();

    match (first.as_deref(), extra) {
        (None, _) | (Some("-h" | "--help"), None) => Command::Help,
        (Some("-V" | "--version"), None) => Command::Version,
        (Some("status"), None) => Command::Status,
        (Some(arg), _) => Command::Invalid(arg.to_string()),
    }
}

fn error_state(error: &ProviderError) -> &'static str {
    match error {
        ProviderError::Unsupported(_) => "Unsupported",
        ProviderError::PermissionDenied(_) => "PermissionDenied",
        ProviderError::BackendUnavailable(_) | ProviderError::Timeout(_) | ProviderError::Dbus(_) => {
            "Unavailable"
        }
        ProviderError::InvalidRequest(_) | ProviderError::Io(_) | ProviderError::Internal(_) => {
            "Unknown"
        }
    }
}

fn print_observation<T>(label: &str, result: Result<T, ProviderError>) -> bool
where
    T: Debug,
{
    match result {
        Ok(value) => {
            println!("{label}: {value:?}");
            true
        }
        Err(error) => {
            println!("{label}: {} ({error})", error_state(&error));
            false
        }
    }
}

async fn status() -> ExitCode {
    let connection = match zbus::Connection::session().await {
        Ok(connection) => connection,
        Err(error) => {
            eprintln!("orbisctl: cannot connect to the user session bus: {error}");
            return ExitCode::FAILURE;
        }
    };

    let battery = SessionChargeLimitProvider::new(ZbusSessionChargeLimitSource::new(
        connection.clone(),
    ));
    let performance = SessionPerformanceProvider::new(ZbusSessionPerformanceSource::new(
        connection.clone(),
    ));
    let gpu_power = SessionGpuPowerProvider::new(ZbusSessionGpuSource::new(connection.clone()));
    let gpu_mux = SessionGpuMuxProvider::new(ZbusSessionGpuSource::new(connection.clone()));
    let gpu_access = SessionGpuAccessProvider::new(ZbusSessionGpuSource::new(connection));

    let mut successful_reads = 0usize;

    if print_observation("battery.charge_limit", battery.charge_limit().await) {
        successful_reads += 1;
    }
    if print_observation("performance.current", performance.current_profile().await) {
        successful_reads += 1;
    }
    if print_observation("performance.available", performance.profiles().await) {
        successful_reads += 1;
    }
    if print_observation("gpu.power", gpu_power.power_state().await) {
        successful_reads += 1;
    }
    if print_observation("gpu.mux", gpu_mux.mux_state().await) {
        successful_reads += 1;
    }
    if print_observation("gpu.access", gpu_access.access_policy().await) {
        successful_reads += 1;
    }

    if successful_reads == 0 {
        eprintln!("orbisctl: no Session1 reads succeeded");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    match parse_args(std::env::args().skip(1)) {
        Command::Help => {
            print!("{HELP}");
            ExitCode::SUCCESS
        }
        Command::Version => {
            println!("orbisctl {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Command::Status => status().await,
        Command::Invalid(arg) => {
            eprintln!("orbisctl: unknown or invalid argument: {arg}");
            eprintln!("Try 'orbisctl --help'.");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_accepts_help_version_and_status() {
        assert_eq!(parse_args(Vec::<String>::new()), Command::Help);
        assert_eq!(parse_args(["--help".into()]), Command::Help);
        assert_eq!(parse_args(["--version".into()]), Command::Version);
        assert_eq!(parse_args(["status".into()]), Command::Status);
    }

    #[test]
    fn parser_rejects_unknown_or_extra_arguments() {
        assert_eq!(
            parse_args(["wat".into()]),
            Command::Invalid("wat".into())
        );
        assert_eq!(
            parse_args(["status".into(), "extra".into()]),
            Command::Invalid("status".into())
        );
    }

    #[test]
    fn provider_errors_are_rendered_without_fake_values() {
        assert_eq!(
            error_state(&ProviderError::Unsupported("x".into())),
            "Unsupported"
        );
        assert_eq!(
            error_state(&ProviderError::PermissionDenied("x".into())),
            "PermissionDenied"
        );
        assert_eq!(
            error_state(&ProviderError::BackendUnavailable("x".into())),
            "Unavailable"
        );
        assert_eq!(
            error_state(&ProviderError::Internal("x".into())),
            "Unknown"
        );
    }
}
