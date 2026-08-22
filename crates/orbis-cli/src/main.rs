use std::fmt::Debug;
use std::future::Future;
use std::io::{self, Write};
use std::process::ExitCode;
use std::time::Duration;

use async_trait::async_trait;
use orbis_core::battery::{BatteryThresholdEvidence, ChargeLimit};
use orbis_core::capability::CapabilityStatus;
use orbis_core::gpu::{GpuAccessPolicy, GpuMuxState, GpuPowerState};
use orbis_core::profile::PerformanceProfile;
use orbis_providers::asus_gpu_mode::{AsusGpuModeSnapshot, AsusdGpuModeProvider};
use orbis_providers::bounded_operation;
use orbis_providers::bounded_provider_call;
use orbis_providers::error::ProviderError;
use orbis_providers::traits::{
    BatteryProvider, GpuAccessProvider, GpuMuxProvider, GpuPowerProvider, PerformanceProvider,
};
use orbis_session_client::{
    HardwarePerformanceSource, SessionChargeLimitProvider, SessionGpuAccessProvider,
    SessionGpuMuxProvider, SessionGpuPowerProvider, SessionPerformanceProvider,
    ZbusHardwarePerformanceSource, ZbusSessionChargeLimitSource, ZbusSessionGpuSource,
    ZbusSessionPerformanceSource, performance_current_from_wire,
};
use serde::Serialize;

const STATUS_SCHEMA_VERSION: u32 = 3;
const HELP: &str = "orbisctl — Orbis Control command-line client\n\nUSAGE:\n    orbisctl [OPTIONS] <COMMAND>\n\nOPTIONS:\n    -h, --help       Print help\n    -V, --version    Print version\n\nCOMMANDS:\n    status [--json]  Read current Session1 state without performing mutations\n    validate platform-profile [--profile PROFILE] [--apply-test]\n                    Validate the Hardware1 profile path (dry-run by default)\n\nVALIDATION:\n    --apply-test     Opt into one controlled profile write and restore\n    --profile NAME   Target quiet, balanced, or performance\n                    Interactive confirmation is required before any write\n";

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
    ValidatePlatformProfile {
        apply_test: bool,
        target: Option<PerformanceProfile>,
    },
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
        [command, subcommand, rest @ ..]
            if command == "validate" && subcommand == "platform-profile" =>
        {
            let mut apply_test = false;
            let mut target = None;
            let mut index = 0;
            while index < rest.len() {
                match rest[index].as_str() {
                    "--apply-test" if !apply_test => apply_test = true,
                    "--profile" if target.is_none() && index + 1 < rest.len() => {
                        index += 1;
                        match PerformanceProfile::parse(&rest[index]) {
                            Ok(profile) => target = Some(profile),
                            Err(_) => return Command::Invalid("validate".into()),
                        }
                    }
                    _ => return Command::Invalid("validate".into()),
                }
                index += 1;
            }
            Command::ValidatePlatformProfile { apply_test, target }
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
    threshold_evidence: Observation<BatteryThresholdEvidence>,
}

#[derive(Debug, Serialize)]
struct PerformanceStatusSnapshot {
    current: Observation<PerformanceProfile>,
    available: Observation<Vec<PerformanceProfile>>,
}

#[derive(Debug, Serialize)]
struct GpuStatusSnapshot {
    product_mode: Observation<AsusGpuModeSnapshot>,
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
            self.battery.threshold_evidence.is_available(),
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
    print_observation(
        "battery.threshold_evidence",
        &snapshot.battery.threshold_evidence,
    );
    print_observation("performance.current", &snapshot.performance.current);
    print_observation("performance.available", &snapshot.performance.available);
    print_observation("gpu.product_mode", &snapshot.gpu.product_mode);
    print_observation("gpu.power", &snapshot.gpu.power);
    print_observation("gpu.mux", &snapshot.gpu.mux);
    print_observation("gpu.access", &snapshot.gpu.access);
}

async fn collect_status() -> Result<StatusSnapshot, zbus::Error> {
    let connection = zbus::Connection::session().await?;
    let system_connection = zbus::Connection::system().await?;

    let battery =
        SessionChargeLimitProvider::new(ZbusSessionChargeLimitSource::new(connection.clone()));
    let performance =
        SessionPerformanceProvider::new(ZbusSessionPerformanceSource::new(connection.clone()));
    let gpu_power = SessionGpuPowerProvider::new(ZbusSessionGpuSource::new(connection.clone()));
    let gpu_mux = SessionGpuMuxProvider::new(ZbusSessionGpuSource::new(connection.clone()));
    let gpu_access = SessionGpuAccessProvider::new(ZbusSessionGpuSource::new(connection.clone()));

    let product_mode = observation_from_result(
        bounded_operation(
            Duration::from_secs(1),
            "asusd",
            "gpu.product_mode",
            AsusdGpuModeProvider::new(system_connection).read_snapshot(),
        )
        .await,
    );

    let charge_limit = observation_from_result(
        bounded_provider_call(&battery, "battery.charge_limit", battery.charge_limit()).await,
    );
    let threshold_evidence = observation_from_result(
        bounded_provider_call(
            &battery,
            "battery.threshold_evidence",
            battery.threshold_evidence(),
        )
        .await,
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
        battery: BatteryStatusSnapshot {
            charge_limit,
            threshold_evidence,
        },
        performance: PerformanceStatusSnapshot { current, available },
        gpu: GpuStatusSnapshot {
            product_mode,
            power,
            mux,
            access,
        },
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

/// Read/write boundary used by the validation workflow.
#[async_trait]
trait PlatformProfileValidationBackend: Send + Sync {
    async fn current(&self) -> Result<PerformanceProfile, ProviderError>;
    async fn choices(&self) -> Result<Vec<PerformanceProfile>, ProviderError>;
    fn write_capability(&self) -> CapabilityStatus;
    async fn apply(&self, profile: PerformanceProfile)
    -> Result<PerformanceProfile, ProviderError>;
}

/// Sanitized validation log. It contains profile symbols only; no bus names,
/// sender identities, paths or user data are recorded.
#[derive(Debug, Default, PartialEq, Eq)]
struct PlatformProfileValidationReport {
    initial_state: Option<PerformanceProfile>,
    available_choices: Vec<PerformanceProfile>,
    write_capability: Option<CapabilityStatus>,
    requested_state: Option<PerformanceProfile>,
    applied_state: Option<PerformanceProfile>,
    read_back: Option<PerformanceProfile>,
    restore_state: Option<PerformanceProfile>,
    error: Option<String>,
    restore_failure: Option<String>,
    mutated: bool,
}

impl PlatformProfileValidationReport {
    fn successful(&self, apply_test: bool) -> bool {
        self.error.is_none()
            && self.restore_failure.is_none()
            && (!apply_test || (self.mutated && self.restore_state == self.initial_state))
    }
}

fn validation_detail(error: ProviderError) -> String {
    error.to_string()
}

fn print_validation_evidence(report: &PlatformProfileValidationReport) {
    println!("initial state: {:?}", report.initial_state);
    println!("available choices: {:?}", report.available_choices);
    println!("write capability: {:?}", report.write_capability);
}

fn print_validation_report(report: &PlatformProfileValidationReport) {
    println!("requested state: {:?}", report.requested_state);
    println!("applied state: {:?}", report.applied_state);
    println!("read-back: {:?}", report.read_back);
    println!("restore state: {:?}", report.restore_state);
    if let Some(error) = &report.restore_failure {
        println!("result: CRITICAL RESTORE FAILURE ({error})");
    } else if let Some(error) = &report.error {
        println!("result: FAILED ({error})");
    } else if report.mutated {
        println!("result: PASS (profile applied, verified, and restored)");
    } else {
        println!("result: DRY-RUN (no mutation performed)");
    }
}

/// Execute the explicit validation workflow. The confirmation callback is
/// called only after initial evidence is read and printed by the caller.
async fn run_platform_profile_validation<B, F>(
    backend: &B,
    apply_test: bool,
    requested: Option<PerformanceProfile>,
    confirm: F,
) -> PlatformProfileValidationReport
where
    B: PlatformProfileValidationBackend,
    F: FnOnce(&PlatformProfileValidationReport) -> bool,
{
    let mut report = PlatformProfileValidationReport::default();
    report.initial_state = match backend.current().await {
        Ok(profile) => Some(profile),
        Err(error) => {
            report.error = Some(format!(
                "initial profile read failed: {}",
                validation_detail(error)
            ));
            return report;
        }
    };
    report.available_choices = match backend.choices().await {
        Ok(choices) => choices,
        Err(error) => {
            report.error = Some(format!(
                "profile choices read failed: {}",
                validation_detail(error)
            ));
            return report;
        }
    };

    let initial = report.initial_state.expect("initial state was recorded");
    if !report.available_choices.contains(&initial) {
        report.error = Some("initial profile is absent from available choices".into());
        return report;
    }
    let target = requested.or_else(|| {
        report
            .available_choices
            .iter()
            .copied()
            .find(|profile| *profile != initial)
    });
    report.requested_state = target;
    let Some(target) = target else {
        report.error = Some("no alternate profile is available for validation".into());
        return report;
    };
    if !report.available_choices.contains(&target) {
        report.error = Some("requested profile is absent from available choices".into());
        return report;
    }
    report.write_capability = Some(backend.write_capability());
    if !apply_test {
        let _ = confirm(&report);
        return report;
    }
    if backend.write_capability() != CapabilityStatus::Supported {
        report.error = Some(format!(
            "write capability is not Supported: {:?}",
            backend.write_capability()
        ));
        return report;
    }
    if !confirm(&report) {
        report.error = Some("explicit APPLY-TEST confirmation was not provided".into());
        return report;
    }

    report.mutated = true;
    match backend.apply(target).await {
        Ok(applied) => report.applied_state = Some(applied),
        Err(error) => {
            report.error = Some(format!(
                "profile apply failed: {}",
                validation_detail(error)
            ))
        }
    }
    if report.error.is_none() {
        match backend.current().await {
            Ok(read_back) if read_back == target => report.read_back = Some(read_back),
            Ok(read_back) => {
                report.read_back = Some(read_back);
                report.error = Some("apply read-back mismatch".into());
            }
            Err(error) => {
                report.error = Some(format!(
                    "apply read-back failed: {}",
                    validation_detail(error)
                ));
            }
        }
    }

    match backend.apply(initial).await {
        Ok(restored) => report.restore_state = Some(restored),
        Err(error) => {
            report.restore_failure = Some(format!(
                "restore apply failed: {}",
                validation_detail(error)
            ));
            return report;
        }
    }
    match backend.current().await {
        Ok(read_back) if read_back == initial => {}
        Ok(read_back) => {
            report.restore_failure = Some(format!(
                "restore read-back mismatch: expected {initial:?}, got {read_back:?}"
            ));
        }
        Err(error) => {
            report.restore_failure = Some(format!(
                "restore read-back failed: {}",
                validation_detail(error)
            ));
        }
    }
    report
}

struct LivePlatformProfileValidationBackend {
    session: SessionPerformanceProvider<ZbusSessionPerformanceSource>,
    hardware: ZbusHardwarePerformanceSource,
    write_capability: CapabilityStatus,
}

#[async_trait]
impl PlatformProfileValidationBackend for LivePlatformProfileValidationBackend {
    async fn current(&self) -> Result<PerformanceProfile, ProviderError> {
        bounded_provider_call(
            &self.session,
            "validation.platform_profile.current",
            self.session.current_profile(),
        )
        .await
    }

    async fn choices(&self) -> Result<Vec<PerformanceProfile>, ProviderError> {
        bounded_provider_call(
            &self.session,
            "validation.platform_profile.choices",
            self.session.profiles(),
        )
        .await
    }

    fn write_capability(&self) -> CapabilityStatus {
        self.write_capability
    }

    async fn apply(
        &self,
        profile: PerformanceProfile,
    ) -> Result<PerformanceProfile, ProviderError> {
        let confirmed = self
            .hardware
            .set_performance(orbis_hardwared::profile_to_wire(profile))
            .await?;
        performance_current_from_wire(confirmed)
    }
}

/// Deadline for one D-Bus bus connection attempt in `orbisctl validate`.
///
/// Bounds socket connect + authentication + hello. Expiry is an honest
/// connection failure (exit FAILURE), never success and never retried.
const VALIDATE_BUS_CONNECT_DEADLINE: Duration = Duration::from_secs(5);

/// Deadline for one read-only Hardware1 status query in `orbisctl validate`.
///
/// Mirrors the bounded Hardware1 status requery deadline used by the GUI
/// composition. Expiry maps to [`CapabilityStatus::Unknown`] — the honest
/// no-evidence state these queries already use for every transport error.
const VALIDATE_STATUS_DEADLINE: Duration = Duration::from_secs(2);

/// Run one read-only Hardware1 status query within an explicit deadline.
///
/// A timeout is never reported as success and never retried; the proven
/// statuses (`Supported`, `Unsupported`, `PermissionDenied`, ...) pass through
/// unchanged so evidence classes are never mixed.
async fn bounded_validate_status<F>(operation: &'static str, future: F) -> CapabilityStatus
where
    F: Future<Output = CapabilityStatus>,
{
    // The status queries resolve to a plain status rather than
    // `Result<_, ProviderError>`; adapt them so the canonical bounded
    // primitive owns the deadline.
    let outcome = bounded_operation(
        VALIDATE_STATUS_DEADLINE,
        "orbisctl-validate",
        operation,
        async move { Ok::<_, ProviderError>(future.await) },
    )
    .await;
    match outcome {
        Ok(status) => status,
        Err(_) => CapabilityStatus::Unknown,
    }
}

async fn validate_platform_profile(
    apply_test: bool,
    requested: Option<PerformanceProfile>,
) -> ExitCode {
    // Every stage before the interactive/mutation section is explicitly
    // bounded (#123): a hung bus or service must fail the command within its
    // deadline instead of blocking forever. Connection expiry is reported as a
    // timeout and exits with FAILURE — never as success.
    let session_connection = match bounded_operation(
        VALIDATE_BUS_CONNECT_DEADLINE,
        "orbisctl-validate",
        "session_bus_connect",
        async {
            zbus::Connection::session()
                .await
                .map_err(|error| ProviderError::Internal(error.to_string()))
        },
    )
    .await
    {
        Ok(connection) => connection,
        Err(error) => {
            eprintln!("orbisctl: cannot connect to the user session bus: {error}");
            return ExitCode::FAILURE;
        }
    };
    let system_connection = match bounded_operation(
        VALIDATE_BUS_CONNECT_DEADLINE,
        "orbisctl-validate",
        "system_bus_connect",
        async {
            zbus::Connection::system()
                .await
                .map_err(|error| ProviderError::Internal(error.to_string()))
        },
    )
    .await
    {
        Ok(connection) => connection,
        Err(error) => {
            eprintln!("orbisctl: cannot connect to the system bus: {error}");
            return ExitCode::FAILURE;
        }
    };
    let write_capability = bounded_validate_status(
        "performance_mutation_status",
        orbis_session_client::hardware1_performance_mutation_status(&system_connection),
    )
    .await;
    let backend = LivePlatformProfileValidationBackend {
        session: SessionPerformanceProvider::new(ZbusSessionPerformanceSource::new(
            session_connection,
        )),
        hardware: ZbusHardwarePerformanceSource::new(system_connection),
        write_capability,
    };
    let report = run_platform_profile_validation(&backend, apply_test, requested, |evidence| {
        print_validation_evidence(evidence);
        if !apply_test {
            return false;
        }
        print!("Type APPLY-TEST to apply one profile and restore it: ");
        let _ = io::stdout().flush();
        let mut input = String::new();
        io::stdin().read_line(&mut input).is_ok() && input.trim() == "APPLY-TEST"
    })
    .await;
    print_validation_report(&report);
    if report.successful(apply_test) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
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
        Command::ValidatePlatformProfile { apply_test, target } => {
            validate_platform_profile(apply_test, target).await
        }
        Command::Invalid(arg) => {
            eprintln!("orbisctl: unknown or invalid argument: {arg}");
            eprintln!("Try 'orbisctl --help'.");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use super::*;

    #[tokio::test(start_paused = true)]
    async fn hung_validate_status_resolves_unknown_within_deadline() {
        let started = tokio::time::Instant::now();
        let status =
            bounded_validate_status("performance_mutation_status", std::future::pending()).await;

        assert_eq!(status, CapabilityStatus::Unknown);
        // The paused Tokio clock advances exactly to the deadline when the
        // timeout fires, so equality proves the bound is actually applied.
        assert_eq!(started.elapsed(), VALIDATE_STATUS_DEADLINE);
    }

    #[tokio::test(start_paused = true)]
    async fn validate_status_timeout_is_never_success_and_never_retried() {
        let calls = Cell::new(0u32);
        let status = bounded_validate_status("performance_mutation_status", async {
            calls.set(calls.get() + 1);
            std::future::pending::<CapabilityStatus>().await
        })
        .await;

        assert_eq!(status, CapabilityStatus::Unknown);
        assert_eq!(calls.get(), 1, "timeout must not retry the query");
    }

    #[tokio::test]
    async fn proven_validate_statuses_pass_through_without_coercion() {
        let proven = [
            CapabilityStatus::Supported,
            CapabilityStatus::Unsupported,
            CapabilityStatus::PermissionDenied,
        ];
        for expected in proven {
            let observed =
                bounded_validate_status("performance_mutation_status", async move { expected })
                    .await;
            assert_eq!(observed, expected);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn hung_bus_connect_surfaces_as_timeout_error_not_success() {
        let result: Result<(), ProviderError> = bounded_operation(
            VALIDATE_BUS_CONNECT_DEADLINE,
            "orbisctl-validate",
            "session_bus_connect",
            async { std::future::pending().await },
        )
        .await;

        assert!(
            matches!(result, Err(ProviderError::Timeout(_))),
            "a hung bus connect must classify as Timeout, never Ok"
        );
    }

    struct FakeValidationBackend {
        current: Mutex<PerformanceProfile>,
        choices: Vec<PerformanceProfile>,
        capability: CapabilityStatus,
        applies: Mutex<VecDeque<Result<PerformanceProfile, ProviderError>>>,
        writes: Mutex<usize>,
    }

    impl FakeValidationBackend {
        fn new(
            capability: CapabilityStatus,
            applies: impl IntoIterator<Item = Result<PerformanceProfile, ProviderError>>,
        ) -> Self {
            Self {
                current: Mutex::new(PerformanceProfile::Silent),
                choices: vec![
                    PerformanceProfile::Silent,
                    PerformanceProfile::Balanced,
                    PerformanceProfile::Turbo,
                ],
                capability,
                applies: Mutex::new(applies.into_iter().collect()),
                writes: Mutex::new(0),
            }
        }

        fn writes(&self) -> usize {
            *self.writes.lock().unwrap()
        }
    }

    #[async_trait]
    impl PlatformProfileValidationBackend for FakeValidationBackend {
        async fn current(&self) -> Result<PerformanceProfile, ProviderError> {
            Ok(*self.current.lock().unwrap())
        }

        async fn choices(&self) -> Result<Vec<PerformanceProfile>, ProviderError> {
            Ok(self.choices.clone())
        }

        fn write_capability(&self) -> CapabilityStatus {
            self.capability
        }

        async fn apply(
            &self,
            profile: PerformanceProfile,
        ) -> Result<PerformanceProfile, ProviderError> {
            *self.writes.lock().unwrap() += 1;
            let result = self
                .applies
                .lock()
                .unwrap()
                .pop_front()
                .expect("scripted apply result");
            if let Ok(confirmed) = result {
                *self.current.lock().unwrap() = confirmed;
                assert_eq!(confirmed, profile);
                Ok(confirmed)
            } else {
                result
            }
        }
    }

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
        assert_eq!(
            parse_args([
                "validate".into(),
                "platform-profile".into(),
                "--apply-test".into(),
                "--profile".into(),
                "balanced".into(),
            ]),
            Command::ValidatePlatformProfile {
                apply_test: true,
                target: Some(PerformanceProfile::Balanced),
            }
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
                threshold_evidence: Observation::Unknown {
                    detail: "not collected".into(),
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
                product_mode: Observation::Unknown {
                    detail: "unknown".into(),
                },
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
        assert_eq!(json["gpu"]["product_mode"]["state"], "unknown");
        assert_eq!(json["gpu"]["access"]["state"], "permission_denied");
        assert_eq!(snapshot.successful_reads(), 2);
    }

    #[tokio::test]
    async fn validation_dry_run_does_not_mutate() {
        let backend = FakeValidationBackend::new(CapabilityStatus::Supported, []);
        let report = run_platform_profile_validation(&backend, false, None, |_| true).await;
        assert!(!report.mutated);
        assert_eq!(backend.writes(), 0);
        assert!(report.successful(false));
    }

    #[tokio::test]
    async fn validation_apply_requires_explicit_apply_test_flag() {
        let backend = FakeValidationBackend::new(
            CapabilityStatus::Supported,
            [Ok(PerformanceProfile::Balanced)],
        );
        let report = run_platform_profile_validation(
            &backend,
            false,
            Some(PerformanceProfile::Balanced),
            |_| true,
        )
        .await;
        assert!(!report.mutated);
        assert_eq!(backend.writes(), 0);
    }

    #[tokio::test]
    async fn unknown_write_capability_blocks_apply_without_mutation() {
        let backend = FakeValidationBackend::new(
            CapabilityStatus::Unknown,
            [Ok(PerformanceProfile::Balanced)],
        );
        let report = run_platform_profile_validation(
            &backend,
            true,
            Some(PerformanceProfile::Balanced),
            |_| panic!("confirmation must not be requested without write evidence"),
        )
        .await;
        assert!(report.error.is_some());
        assert!(!report.mutated);
        assert_eq!(backend.writes(), 0);
    }

    #[tokio::test]
    async fn failed_write_does_not_claim_success() {
        let backend = FakeValidationBackend::new(
            CapabilityStatus::Supported,
            [
                Err(ProviderError::BackendUnavailable("write failed".into())),
                Ok(PerformanceProfile::Silent),
            ],
        );
        let report = run_platform_profile_validation(
            &backend,
            true,
            Some(PerformanceProfile::Balanced),
            |_| true,
        )
        .await;
        assert!(report.error.is_some());
        assert!(!report.successful(true));
        assert_eq!(report.restore_state, Some(PerformanceProfile::Silent));
        assert_eq!(backend.writes(), 2);
    }

    #[tokio::test]
    async fn restore_failure_is_critical_and_separate() {
        let backend = FakeValidationBackend::new(
            CapabilityStatus::Supported,
            [
                Ok(PerformanceProfile::Balanced),
                Err(ProviderError::PermissionDenied("restore denied".into())),
            ],
        );
        let report = run_platform_profile_validation(
            &backend,
            true,
            Some(PerformanceProfile::Balanced),
            |_| true,
        )
        .await;
        assert!(report.restore_failure.is_some());
        assert!(!report.successful(true));
    }
}
