use std::fmt::Debug;
use std::process::ExitCode;

use orbis_core::battery::ChargeLimit;
use orbis_core::gpu::{GpuAccessPolicy, GpuMuxState, GpuPowerState};
use orbis_core::profile::PerformanceProfile;
use orbis_providers::bounded_provider_call;
use orbis_providers::error::ProviderError;
use orbis_providers::traits::{
    BatteryProvider, GpuAccessProvider, GpuMuxProvider, GpuPowerProvider, PerformanceProvider,
};
use orbis_session_client::{
    SessionChargeLimitProvider, SessionGpuAccessProvider, SessionGpuMuxProvider,
    SessionGpuPowerProvider, SessionPerformanceProvider, ZbusSessionChargeLimitSource,
    ZbusSessionGpuSource, ZbusSessionPerformanceSource,
};
use serde::Serialize;

const STATUS_SCHEMA_VERSION: u32 = 1;
const HELP: &str = "orbisctl — Orbis Control read-only command-line client\n\nUSAGE:\n    orbisctl [OPTIONS] <COMMAND>\n\nOPTIONS:\n    -h, --help       Print help\n    -V, --version    Print version\n\nCOMMANDS:\n    status [--json]  Read current Session1 state without performing mutations\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Human,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Command {
    Help,
    Version,
    Status(OutputFormat),
    Invalid(String),
}

fn parse_args<I>(args: I) -> Command
where
    I: IntoIterator<Item = String>,
{
    let args: Vec<String> = args.into_iter().collect();
    match args.as_slice() {
        [] => Command::Help,
        [arg] if matches!(arg.as_str(), "-h" | "--help") => Command::Help,
        [arg] if matches!(arg.as_str(), "-V" | "--version") => Command::Version,
        [arg] if arg == "status" => Command::Status(OutputFormat::Human),
        [command, flag] if command == "status" && flag == "--json" => {
            Command::Status(OutputFormat::Json)
        }
        [arg, ..] => Command::Invalid(arg.clone()),
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum Observation<T> {
    Available { value: T },
    Unsupported { detail: String },
    PermissionDenied { detail: String },
    Unavailable { detail: String },
    Unknown { detail: String },
}

impl<T> Observation<T> {
    fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }
}

fn observation_from_result<T>(result: Result<T, ProviderError>) -> Observation<T> {
    match result {
        Ok(value) => Observation::Available { value },
        Err(error) => {
            let detail = error.to_string();
            match error {
                ProviderError::Unsupported(_) => Observation::Unsupported { detail },
                ProviderError::PermissionDenied(_) => Observation::PermissionDenied { detail },
                ProviderError::BackendUnavailable(_)
                | ProviderError::Timeout(_)
                | ProviderError::Dbus(_) => Observation::Unavailable { detail },
                ProviderError::InvalidRequest(_)
                | ProviderError::Io(_)
                | ProviderError::Internal(_)
                | ProviderError::Conflict(_) => Observation::Unknown { detail },
            }
        }
    }
}

#[derive(Debug, Serialize)]
struct BatteryStatusSnapshot {
    charge_limit: Observation<ChargeLimit>,
}

#[derive(Debug, Serialize)]
struct PerformanceStatusSnapshot {
    current: Observation<PerformanceProfile>,
    available: Observation<Vec<PerformanceProfile>>,
}

#[derive(Debug, Serialize)]
struct GpuStatusSnapshot {
    power: Observation<GpuPowerState>,
    mux: Observation<GpuMuxState>,
    access: Observation<GpuAccessPolicy>,
}

#[derive(Debug, Serialize)]
struct StatusSnapshot {
    schema_version: u32,
    battery: BatteryStatusSnapshot,
    performance: PerformanceStatusSnapshot,
    gpu: GpuStatusSnapshot,
}

impl StatusSnapshot {
    fn successful_reads(&self) -> usize {
        [
            self.battery.charge_limit.is_available(),
            self.performance.current.is_available(),
            self.performance.available.is_available(),
            self.gpu.power.is_available(),
            self.gpu.mux.is_available(),
            self.gpu.access.is_available(),
        ]
        .into_iter()
        .filter(|available| *available)
        .count()
    }
}

fn print_observation<T>(label: &str, observation: &Observation<T>)
where
    T: Debug,
{
    match observation {
        Observation::Available { value } => println!("{label}: {value:?}"),
        Observation::Unsupported { detail } => {
            println!("{label}: Unsupported ({detail})")
        }
        Observation::PermissionDenied { detail } => {
            println!("{label}: PermissionDenied ({detail})")
        }
        Observation::Unavailable { detail } => {
            println!("{label}: Unavailable ({detail})")
        }
        Observation::Unknown { detail } => println!("{label}: Unknown ({detail})"),
    }
}

fn print_human(snapshot: &StatusSnapshot) {
    print_observation("battery.charge_limit", &snapshot.battery.charge_limit);
    print_observation("performance.current", &snapshot.performance.current);
    print_observation("performance.available", &snapshot.performance.available);
    print_observation("gpu.power", &snapshot.gpu.power);
    print_observation("gpu.mux", &snapshot.gpu.mux);
    print_observation("gpu.access", &snapshot.gpu.access);
}

async fn collect_status() -> Result<StatusSnapshot, zbus::Error> {
    let connection = zbus::Connection::session().await?;

    let battery =
        SessionChargeLimitProvider::new(ZbusSessionChargeLimitSource::new(connection.clone()));
    let performance =
        SessionPerformanceProvider::new(ZbusSessionPerformanceSource::new(connection.clone()));
    let gpu_power = SessionGpuPowerProvider::new(ZbusSessionGpuSource::new(connection.clone()));
    let gpu_mux = SessionGpuMuxProvider::new(ZbusSessionGpuSource::new(connection.clone()));
    let gpu_access = SessionGpuAccessProvider::new(ZbusSessionGpuSource::new(connection));

    let charge_limit = observation_from_result(
        bounded_provider_call(&battery, "battery.charge_limit", battery.charge_limit()).await,
    );

    let current = observation_from_result(
        bounded_provider_call(
            &performance,
            "performance.current",
            performance.current_profile(),
        )
        .await,
    );
    let available = observation_from_result(
        bounded_provider_call(
            &performance,
            "performance.available",
            performance.profiles(),
        )
        .await,
    );

    let power = observation_from_result(
        bounded_provider_call(&gpu_power, "gpu.power", gpu_power.power_state()).await,
    );

    let mux = observation_from_result(
        bounded_provider_call(&gpu_mux, "gpu.mux", gpu_mux.mux_state()).await,
    );

    let access = observation_from_result(
        bounded_provider_call(&gpu_access, "gpu.access", gpu_access.access_policy()).await,
    );

    Ok(StatusSnapshot {
        schema_version: STATUS_SCHEMA_VERSION,
        battery: BatteryStatusSnapshot { charge_limit },
        performance: PerformanceStatusSnapshot { current, available },
        gpu: GpuStatusSnapshot { power, mux, access },
    })
}

async fn status(format: OutputFormat) -> ExitCode {
    let snapshot = match collect_status().await {
        Ok(snapshot) => snapshot,
        Err(error) => {
            eprintln!("orbisctl: cannot connect to the user session bus: {error}");
            return ExitCode::FAILURE;
        }
    };

    match format {
        OutputFormat::Human => print_human(&snapshot),
        OutputFormat::Json => match serde_json::to_string_pretty(&snapshot) {
            Ok(json) => println!("{json}"),
            Err(error) => {
                eprintln!("orbisctl: cannot serialize status JSON: {error}");
                return ExitCode::FAILURE;
            }
        },
    }

    if snapshot.successful_reads() == 0 {
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
        Command::Status(format) => status(format).await,
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
    fn parser_accepts_help_version_and_status_formats() {
        assert_eq!(parse_args(Vec::<String>::new()), Command::Help);
        assert_eq!(parse_args(["--help".into()]), Command::Help);
        assert_eq!(parse_args(["--version".into()]), Command::Version);
        assert_eq!(
            parse_args(["status".into()]),
            Command::Status(OutputFormat::Human)
        );
        assert_eq!(
            parse_args(["status".into(), "--json".into()]),
            Command::Status(OutputFormat::Json)
        );
    }

    #[test]
    fn parser_rejects_unknown_or_extra_arguments() {
        assert_eq!(parse_args(["wat".into()]), Command::Invalid("wat".into()));
        assert_eq!(
            parse_args(["status".into(), "extra".into()]),
            Command::Invalid("status".into())
        );
        assert_eq!(
            parse_args(["status".into(), "--json".into(), "extra".into()]),
            Command::Invalid("status".into())
        );
    }

    #[test]
    fn provider_errors_are_classified_without_fake_values() {
        assert!(matches!(
            observation_from_result::<()>(Err(ProviderError::Unsupported("x".into()))),
            Observation::Unsupported { .. }
        ));
        assert!(matches!(
            observation_from_result::<()>(Err(ProviderError::PermissionDenied("x".into()))),
            Observation::PermissionDenied { .. }
        ));
        assert!(matches!(
            observation_from_result::<()>(Err(ProviderError::BackendUnavailable("x".into()))),
            Observation::Unavailable { .. }
        ));
        assert!(matches!(
            observation_from_result::<()>(Err(ProviderError::Timeout("x".into()))),
            Observation::Unavailable { .. }
        ));
        assert!(matches!(
            observation_from_result::<()>(Err(ProviderError::Internal("x".into()))),
            Observation::Unknown { .. }
        ));
    }

    #[test]
    fn json_status_is_versioned_and_preserves_typed_error_state() {
        let snapshot = StatusSnapshot {
            schema_version: STATUS_SCHEMA_VERSION,
            battery: BatteryStatusSnapshot {
                charge_limit: Observation::Unsupported {
                    detail: "not supported".into(),
                },
            },
            performance: PerformanceStatusSnapshot {
                current: Observation::Available {
                    value: PerformanceProfile::Balanced,
                },
                available: Observation::Available {
                    value: vec![PerformanceProfile::Balanced],
                },
            },
            gpu: GpuStatusSnapshot {
                power: Observation::Unavailable {
                    detail: "backend down".into(),
                },
                mux: Observation::Unknown {
                    detail: "unknown".into(),
                },
                access: Observation::PermissionDenied {
                    detail: "denied".into(),
                },
            },
        };

        let json = serde_json::to_value(&snapshot).expect("serialize status");
        assert_eq!(json["schema_version"], STATUS_SCHEMA_VERSION);
        assert_eq!(json["battery"]["charge_limit"]["state"], "unsupported");
        assert_eq!(json["performance"]["current"]["state"], "available");
        assert_eq!(json["performance"]["current"]["value"], "balanced");
        assert_eq!(json["gpu"]["access"]["state"], "permission_denied");
        assert_eq!(snapshot.successful_reads(), 2);
    }
}
