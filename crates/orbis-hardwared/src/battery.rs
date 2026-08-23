//! Internal ASUS battery mutation backend used by the typed Hardware1 service.
//!
//! The only mutation backend is the typed `asusd` D-Bus API; the kernel source
//! is read-only and is used for authoritative effective-threshold evidence.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use orbis_core::action::ApplyResult;
use orbis_providers::error::ProviderError;
use zbus::Connection;
use zbus::names::BusName;

pub const ASUSD_BUS_NAME: &str = "xyz.ljones.Asusd";
pub const ASUSD_OBJECT_PATH: &str = "/xyz/ljones";
pub const ASUSD_INTERFACE: &str = "xyz.ljones.Platform";
pub const EFFECTIVE_THRESHOLD_FILE: &str = "charge_control_end_threshold";
const POWER_SUPPLY_ROOT: &str = "/sys/class/power_supply";
const MIN_CHARGE_LIMIT: u8 = 20;
const MAX_CHARGE_LIMIT: u8 = 100;

/// Validate the public Battery input without contacting any backend.
pub fn validate_charge_limit(percent: u8) -> Result<(), ProviderError> {
    if !(MIN_CHARGE_LIMIT..=MAX_CHARGE_LIMIT).contains(&percent) {
        return Err(ProviderError::InvalidRequest(format!(
            "battery charge limit must be {MIN_CHARGE_LIMIT}..={MAX_CHARGE_LIMIT}, got {percent}"
        )));
    }
    Ok(())
}

/// Typed asusd operations required by the mutation algorithm.
#[async_trait]
pub trait AsusdBatteryClient: Send + Sync {
    async fn set_charge_control_end_threshold(&self, percent: u8) -> Result<(), ProviderError>;
    async fn get_charge_control_end_threshold(&self) -> Result<u8, ProviderError>;

    /// Non-activating runtime owner liveness probe for the fixed asusd name.
    ///
    /// Uses only the D-Bus daemon ownership table (`NameHasOwner`), so a
    /// stopped-but-activatable asusd is never started as a probing side
    /// effect. The default reports a confirmed owner for test fakes.
    async fn asusd_owned(&self) -> Result<bool, ProviderError> {
        Ok(true)
    }
}

/// Fresh read-only effective kernel threshold source.
#[async_trait]
pub trait BatteryEffectiveReader: Send + Sync {
    async fn read_effective_threshold(&self) -> Result<u8, ProviderError>;
}

/// Read-only `/sys/class/power_supply/<native>/charge_control_end_threshold` reader.
pub struct SysfsBatteryEffectiveReader {
    path: PathBuf,
}

fn discovery_read_error(path: &Path, error: std::io::Error) -> ProviderError {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        ProviderError::PermissionDenied(format!("battery discovery cannot read {}", path.display()))
    } else {
        ProviderError::Io(error)
    }
}

fn remember_discovery_error(
    error: ProviderError,
    permission_error: &mut Option<String>,
    io_error: &mut Option<std::io::Error>,
) {
    match error {
        ProviderError::PermissionDenied(detail) => {
            if permission_error.is_none() {
                *permission_error = Some(detail);
            }
        }
        ProviderError::Io(error) if io_error.is_none() => {
            *io_error = Some(error);
        }
        _ => {}
    }
}

fn discover_effective_reader_in(root: &Path) -> Result<SysfsBatteryEffectiveReader, ProviderError> {
    let entries = std::fs::read_dir(root).map_err(ProviderError::Io)?;
    let mut permission_error = None;
    let mut io_error = None;

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                if io_error.is_none() {
                    io_error = Some(error);
                }
                continue;
            }
        };

        let type_path = entry.path().join("type");
        let threshold_path = entry.path().join(EFFECTIVE_THRESHOLD_FILE);
        let type_value = match std::fs::read_to_string(&type_path) {
            Ok(value) => value,
            Err(error) => {
                remember_discovery_error(
                    discovery_read_error(&type_path, error),
                    &mut permission_error,
                    &mut io_error,
                );
                continue;
            }
        };

        if !type_value.trim().eq_ignore_ascii_case("battery") {
            continue;
        }

        match std::fs::metadata(&threshold_path) {
            Ok(metadata) if metadata.is_file() => {
                return Ok(SysfsBatteryEffectiveReader {
                    path: threshold_path,
                });
            }
            Ok(_) => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                remember_discovery_error(
                    discovery_read_error(&threshold_path, error),
                    &mut permission_error,
                    &mut io_error,
                );
            }
        }
    }

    if let Some(detail) = permission_error {
        return Err(ProviderError::PermissionDenied(detail));
    }
    if let Some(error) = io_error {
        return Err(ProviderError::Io(error));
    }

    Err(ProviderError::Unsupported(
        "no battery effective threshold source discovered".into(),
    ))
}

/// Discover a power-supply battery exposing the effective threshold attribute.
///
/// Discovery is evidence-preserving: a valid candidate wins even if another
/// entry disappeared or was unreadable during the scan. If no candidate is
/// found, permission/I/O failures remain typed failures and only a fully
/// inspected structural absence becomes `Unsupported`.
pub fn discover_effective_reader() -> Result<SysfsBatteryEffectiveReader, ProviderError> {
    discover_effective_reader_in(Path::new(POWER_SUPPLY_ROOT))
}

impl SysfsBatteryEffectiveReader {
    /// Production constructor from a validated udev/UPower native power-supply name.
    pub fn from_native_path(native_path: &str) -> Result<Self, ProviderError> {
        let component = Path::new(native_path);
        if native_path.is_empty()
            || component.components().count() != 1
            || component.file_name().and_then(|value| value.to_str()) != Some(native_path)
        {
            return Err(ProviderError::InvalidRequest(format!(
                "invalid battery native path: {native_path}"
            )));
        }

        Ok(Self {
            path: PathBuf::from(POWER_SUPPLY_ROOT)
                .join(native_path)
                .join(EFFECTIVE_THRESHOLD_FILE),
        })
    }

    #[cfg(test)]
    fn for_test(path: PathBuf) -> Self {
        Self { path }
    }
}

#[async_trait]
impl BatteryEffectiveReader for SysfsBatteryEffectiveReader {
    async fn read_effective_threshold(&self) -> Result<u8, ProviderError> {
        let raw = std::fs::read_to_string(&self.path).map_err(ProviderError::Io)?;
        let value = raw.trim().parse::<u16>().map_err(|error| {
            ProviderError::Internal(format!(
                "battery effective threshold is not an integer: {error}"
            ))
        })?;
        if value > u16::from(MAX_CHARGE_LIMIT) {
            return Err(ProviderError::Internal(format!(
                "battery effective threshold outside percent range: {value}"
            )));
        }
        Ok(value as u8)
    }
}

#[zbus::proxy(
    interface = "xyz.ljones.Platform",
    default_service = "xyz.ljones.Asusd",
    default_path = "/xyz/ljones"
)]
trait AsusdPlatform {
    #[zbus(property)]
    fn charge_control_end_threshold(&self) -> zbus::Result<u8>;

    #[zbus(property)]
    fn set_charge_control_end_threshold(&self, value: u8) -> zbus::Result<()>;
}

/// Production typed client for the asusd compatibility backend.
pub struct ZbusAsusdBatteryClient {
    connection: Connection,
}

impl ZbusAsusdBatteryClient {
    /// Construct without performing a D-Bus call.
    pub fn new(connection: Connection) -> Self {
        Self { connection }
    }

    async fn proxy(&self) -> Result<AsusdPlatformProxy<'_>, ProviderError> {
        AsusdPlatformProxy::new(&self.connection)
            .await
            .map_err(|error| ProviderError::Dbus(format!("asusd proxy: {error}")))
    }
}

#[async_trait]
impl AsusdBatteryClient for ZbusAsusdBatteryClient {
    async fn set_charge_control_end_threshold(&self, percent: u8) -> Result<(), ProviderError> {
        self.proxy()
            .await?
            .set_charge_control_end_threshold(percent)
            .await
            .map_err(|error| ProviderError::Dbus(format!("asusd setter: {error}")))
    }

    async fn get_charge_control_end_threshold(&self) -> Result<u8, ProviderError> {
        self.proxy()
            .await?
            .charge_control_end_threshold()
            .await
            .map_err(|error| ProviderError::Dbus(format!("asusd configured read: {error}")))
    }

    async fn asusd_owned(&self) -> Result<bool, ProviderError> {
        let daemon = zbus::fdo::DBusProxy::new(&self.connection)
            .await
            .map_err(|error| ProviderError::Dbus(format!("system D-Bus daemon proxy: {error}")))?;
        let bus_name = BusName::try_from(ASUSD_BUS_NAME).map_err(|error| {
            ProviderError::Internal(format!("invalid fixed asusd D-Bus name: {error}"))
        })?;
        daemon
            .name_has_owner(bus_name)
            .await
            .map_err(|error| match error {
                zbus::fdo::Error::AccessDenied(message) => ProviderError::PermissionDenied(message),
                other => ProviderError::Dbus(format!("NameHasOwner({ASUSD_BUS_NAME}): {other}")),
            })
    }
}

/// Fresh result of an asusd mutation and its two backend read-backs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatteryMutationReadback {
    pub requested_percent: u8,
    pub configured_percent: u8,
    pub effective_percent: u8,
    pub result: ApplyResult,
}

/// Typed runtime evidence for Battery mutation backend availability.
///
/// This is the honest mutation-path classification produced by hardwared at
/// startup. It deliberately preserves the distinction between a proven
/// backend (`Supported`), a structurally absent mutation capability
/// (`Unsupported`), a temporary discovery failure (`TemporarilyUnavailable`)
/// and an authorization failure (`PermissionDenied`) instead of collapsing
/// everything into a single bool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryMutationStatus {
    /// A proven production mutation backend (effective threshold reader +
    /// typed asusd client) is installed.
    Supported,
    /// Mutation capability is structurally absent (no effective threshold
    /// source discovered, no hardware support).
    Unsupported,
    /// A known/expected backend is temporarily unavailable (startup
    /// discovery failed for a non-structural reason).
    TemporarilyUnavailable,
    /// Mutation exists but current authorization evidence denies it.
    PermissionDenied,
    /// No evidence about mutation availability.
    Unknown,
}

/// Stable wire values for `Hardware1.BatteryMutationStatus`.
pub mod battery_mutation_wire {
    use super::BatteryMutationStatus;

    /// Proven production mutation backend.
    pub const SUPPORTED: u8 = 0;
    /// Mutation capability structurally absent.
    pub const UNSUPPORTED: u8 = 1;
    /// Known backend temporarily unavailable.
    pub const TEMPORARILY_UNAVAILABLE: u8 = 2;
    /// Mutation denied by authorization evidence.
    pub const PERMISSION_DENIED: u8 = 3;
    /// No evidence.
    pub const UNKNOWN: u8 = 4;

    /// Encode typed status into the D-Bus wire value.
    pub fn to_wire(status: BatteryMutationStatus) -> u8 {
        match status {
            BatteryMutationStatus::Supported => SUPPORTED,
            BatteryMutationStatus::Unsupported => UNSUPPORTED,
            BatteryMutationStatus::TemporarilyUnavailable => TEMPORARILY_UNAVAILABLE,
            BatteryMutationStatus::PermissionDenied => PERMISSION_DENIED,
            BatteryMutationStatus::Unknown => UNKNOWN,
        }
    }

    /// Decode a wire value; unknown values produce `None` so callers can
    /// classify them as `Unknown` instead of inventing a known state.
    pub fn from_wire(raw: u8) -> Option<BatteryMutationStatus> {
        match raw {
            SUPPORTED => Some(BatteryMutationStatus::Supported),
            UNSUPPORTED => Some(BatteryMutationStatus::Unsupported),
            TEMPORARILY_UNAVAILABLE => Some(BatteryMutationStatus::TemporarilyUnavailable),
            PERMISSION_DENIED => Some(BatteryMutationStatus::PermissionDenied),
            UNKNOWN => Some(BatteryMutationStatus::Unknown),
            _ => None,
        }
    }
}

#[async_trait]
pub trait BatteryMutationBackend: Send + Sync {
    async fn set_charge_limit(&self, percent: u8)
    -> Result<BatteryMutationReadback, ProviderError>;

    /// Report the typed runtime availability of this mutation backend.
    ///
    /// This is read-only evidence used by capability probing; it never
    /// performs I/O and never mutates hardware.
    fn mutation_status(&self) -> BatteryMutationStatus;

    /// Runtime backend liveness re-check used to demote a stale `Supported`
    /// status (#107). The default keeps the startup status authoritative for
    /// backends without a dynamic probe.
    async fn backend_alive(&self) -> Result<bool, ProviderError> {
        Ok(true)
    }
}

/// Internal compatibility backend; it never writes the kernel directly.
pub struct AsusdBatteryMutationBackend<A, E> {
    asusd: A,
    effective: E,
}

impl<A, E> AsusdBatteryMutationBackend<A, E> {
    pub fn new(asusd: A, effective: E) -> Self {
        Self { asusd, effective }
    }
}

impl<A, E> AsusdBatteryMutationBackend<A, E>
where
    A: AsusdBatteryClient,
    E: BatteryEffectiveReader,
{
    /// Validate, perform one asusd setter, then perform fresh read-backs.
    pub async fn set_charge_limit(
        &self,
        percent: u8,
    ) -> Result<BatteryMutationReadback, ProviderError> {
        validate_charge_limit(percent)?;

        // Exactly one mutation, owned by asusd. No retry and no sysfs fallback.
        self.asusd.set_charge_control_end_threshold(percent).await?;

        let configured = self.asusd.get_charge_control_end_threshold().await?;
        if configured != percent {
            return Err(ProviderError::BackendUnavailable(format!(
                "asusd configured read-back mismatch: expected={percent}, got={configured}"
            )));
        }

        let effective = self.effective.read_effective_threshold().await?;

        // Configured/effective divergence is explicitly allowed by ADR-0007.
        Ok(BatteryMutationReadback {
            requested_percent: percent,
            configured_percent: configured,
            effective_percent: effective,
            result: ApplyResult::Applied,
        })
    }
}

#[async_trait]
impl<A, E> BatteryMutationBackend for AsusdBatteryMutationBackend<A, E>
where
    A: AsusdBatteryClient,
    E: BatteryEffectiveReader,
{
    async fn set_charge_limit(
        &self,
        percent: u8,
    ) -> Result<BatteryMutationReadback, ProviderError> {
        self.set_charge_limit(percent).await
    }

    fn mutation_status(&self) -> BatteryMutationStatus {
        BatteryMutationStatus::Supported
    }

    async fn backend_alive(&self) -> Result<bool, ProviderError> {
        self.asusd.asusd_owned().await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[derive(Clone)]
    struct FakeAsusd {
        configured: Arc<Mutex<u8>>,
        setter_calls: Arc<AtomicUsize>,
        setter_error: Arc<Mutex<Option<String>>>,
        getter_error: Arc<Mutex<Option<String>>>,
        configured_override: Arc<Mutex<Option<u8>>>,
    }

    impl FakeAsusd {
        fn new(configured: u8) -> Self {
            Self {
                configured: Arc::new(Mutex::new(configured)),
                setter_calls: Arc::new(AtomicUsize::new(0)),
                setter_error: Arc::new(Mutex::new(None)),
                getter_error: Arc::new(Mutex::new(None)),
                configured_override: Arc::new(Mutex::new(None)),
            }
        }

        fn setter_calls(&self) -> usize {
            self.setter_calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl AsusdBatteryClient for FakeAsusd {
        async fn set_charge_control_end_threshold(&self, percent: u8) -> Result<(), ProviderError> {
            self.setter_calls.fetch_add(1, Ordering::SeqCst);
            if let Some(error) = self.setter_error.lock().unwrap().clone() {
                return Err(ProviderError::Dbus(error));
            }
            *self.configured.lock().unwrap() = percent;
            Ok(())
        }

        async fn get_charge_control_end_threshold(&self) -> Result<u8, ProviderError> {
            if let Some(error) = self.getter_error.lock().unwrap().clone() {
                return Err(ProviderError::Dbus(error));
            }
            Ok(self
                .configured_override
                .lock()
                .unwrap()
                .unwrap_or(*self.configured.lock().unwrap()))
        }
    }

    #[derive(Clone)]
    struct FakeEffective {
        value: Arc<Mutex<Result<u8, String>>>,
        reads: Arc<AtomicUsize>,
    }

    impl FakeEffective {
        fn new(value: Result<u8, String>) -> Self {
            Self {
                value: Arc::new(Mutex::new(value)),
                reads: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    #[async_trait]
    impl BatteryEffectiveReader for FakeEffective {
        async fn read_effective_threshold(&self) -> Result<u8, ProviderError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            self.value
                .lock()
                .unwrap()
                .clone()
                .map_err(ProviderError::Dbus)
        }
    }

    fn backend(
        asusd: FakeAsusd,
        effective: FakeEffective,
    ) -> AsusdBatteryMutationBackend<FakeAsusd, FakeEffective> {
        AsusdBatteryMutationBackend::new(asusd, effective)
    }

    fn unique_test_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "orbis-hardwared-battery-{name}-{}-{nanos}",
            std::process::id()
        ))
    }

    #[test]
    fn discovery_permission_error_is_preserved() {
        let error = discovery_read_error(
            Path::new("/sys/class/power_supply/BAT0/type"),
            std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        );
        assert!(matches!(error, ProviderError::PermissionDenied(_)));
    }

    #[test]
    fn discovery_io_error_is_preserved() {
        let error = discovery_read_error(
            Path::new("/sys/class/power_supply/BAT0/type"),
            std::io::Error::from(std::io::ErrorKind::WouldBlock),
        );
        assert!(matches!(error, ProviderError::Io(_)));
    }

    #[test]
    fn discovery_broken_entry_does_not_hide_later_valid_battery() {
        let root = unique_test_dir("mixed");
        let broken = root.join("AC0");
        let valid = root.join("BAT9");
        std::fs::create_dir_all(&broken).unwrap();
        std::fs::create_dir_all(&valid).unwrap();
        std::fs::write(valid.join("type"), "Battery\n").unwrap();
        std::fs::write(valid.join(EFFECTIVE_THRESHOLD_FILE), "80\n").unwrap();

        let reader = discover_effective_reader_in(&root).unwrap();
        assert_eq!(reader.path, valid.join(EFFECTIVE_THRESHOLD_FILE));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn discovery_broken_entry_without_candidate_is_not_unsupported() {
        let root = unique_test_dir("broken-only");
        std::fs::create_dir_all(root.join("BAT0")).unwrap();

        let result = discover_effective_reader_in(&root);
        assert!(matches!(result, Err(ProviderError::Io(_))));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn discovery_structural_absence_is_unsupported() {
        let root = unique_test_dir("unsupported");
        let ac = root.join("AC0");
        let battery_without_threshold = root.join("BAT0");
        std::fs::create_dir_all(&ac).unwrap();
        std::fs::create_dir_all(&battery_without_threshold).unwrap();
        std::fs::write(ac.join("type"), "Mains\n").unwrap();
        std::fs::write(battery_without_threshold.join("type"), "Battery\n").unwrap();

        let result = discover_effective_reader_in(&root);
        assert!(matches!(result, Err(ProviderError::Unsupported(_))));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn rejects_19_before_setter() {
        let asusd = FakeAsusd::new(80);
        let result = backend(asusd.clone(), FakeEffective::new(Ok(80)))
            .set_charge_limit(19)
            .await;
        assert!(matches!(result, Err(ProviderError::InvalidRequest(_))));
        assert_eq!(asusd.setter_calls(), 0);
    }

    #[tokio::test]
    async fn rejects_101_before_setter() {
        let asusd = FakeAsusd::new(80);
        let result = backend(asusd.clone(), FakeEffective::new(Ok(80)))
            .set_charge_limit(101)
            .await;
        assert!(matches!(result, Err(ProviderError::InvalidRequest(_))));
        assert_eq!(asusd.setter_calls(), 0);
    }

    #[tokio::test]
    async fn accepts_20_with_exactly_one_setter_and_readbacks() {
        let asusd = FakeAsusd::new(80);
        let result = backend(asusd.clone(), FakeEffective::new(Ok(20)))
            .set_charge_limit(20)
            .await
            .unwrap();
        assert_eq!(asusd.setter_calls(), 1);
        assert_eq!(result.requested_percent, 20);
        assert_eq!(result.configured_percent, 20);
        assert_eq!(result.effective_percent, 20);
        assert_eq!(result.result, ApplyResult::Applied);
    }

    #[tokio::test]
    async fn accepts_100_as_a_regular_threshold() {
        let asusd = FakeAsusd::new(80);
        let result = backend(asusd.clone(), FakeEffective::new(Ok(100)))
            .set_charge_limit(100)
            .await
            .unwrap();
        assert_eq!(asusd.setter_calls(), 1);
        assert_eq!(result.effective_percent, 100);
    }

    #[tokio::test]
    async fn configured_mismatch_is_an_error_without_retry() {
        let asusd = FakeAsusd::new(80);
        *asusd.configured_override.lock().unwrap() = Some(81);
        let result = backend(asusd.clone(), FakeEffective::new(Ok(81)))
            .set_charge_limit(80)
            .await;
        assert!(matches!(result, Err(ProviderError::BackendUnavailable(_))));
        assert_eq!(asusd.setter_calls(), 1);
    }

    #[tokio::test]
    async fn effective_read_failure_is_propagated_without_retry() {
        let asusd = FakeAsusd::new(80);
        let effective = FakeEffective::new(Err("effective unavailable".into()));
        let result = backend(asusd.clone(), effective).set_charge_limit(80).await;
        assert!(matches!(result, Err(ProviderError::Dbus(_))));
        assert_eq!(asusd.setter_calls(), 1);
    }

    #[tokio::test]
    async fn effective_divergence_is_reported_but_allowed() {
        let asusd = FakeAsusd::new(80);
        let result = backend(asusd.clone(), FakeEffective::new(Ok(100)))
            .set_charge_limit(80)
            .await
            .unwrap();
        assert_eq!(result.configured_percent, 80);
        assert_eq!(result.effective_percent, 100);
        assert_eq!(result.result, ApplyResult::Applied);
    }

    #[tokio::test]
    async fn setter_error_is_propagated_without_retry_or_reads() {
        let asusd = FakeAsusd::new(80);
        *asusd.setter_error.lock().unwrap() = Some("setter failed".into());
        let effective = FakeEffective::new(Ok(80));
        let result = backend(asusd.clone(), effective.clone())
            .set_charge_limit(80)
            .await;
        assert!(matches!(result, Err(ProviderError::Dbus(_))));
        assert_eq!(asusd.setter_calls(), 1);
        assert_eq!(effective.reads.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn configured_getter_error_is_propagated_without_effective_read() {
        let asusd = FakeAsusd::new(80);
        *asusd.getter_error.lock().unwrap() = Some("getter failed".into());
        let effective = FakeEffective::new(Ok(80));
        let result = backend(asusd.clone(), effective.clone())
            .set_charge_limit(80)
            .await;
        assert!(matches!(result, Err(ProviderError::Dbus(_))));
        assert_eq!(asusd.setter_calls(), 1);
        assert_eq!(effective.reads.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn sysfs_effective_reader_is_read_only_and_fresh() {
        let directory = unique_test_dir("fresh");
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join(EFFECTIVE_THRESHOLD_FILE);
        std::fs::write(&path, "80\n").unwrap();
        let reader = SysfsBatteryEffectiveReader::for_test(path.clone());
        assert_eq!(reader.read_effective_threshold().await.unwrap(), 80);
        std::fs::write(&path, "100\n").unwrap();
        assert_eq!(reader.read_effective_threshold().await.unwrap(), 100);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn native_path_is_validated_without_bat1_assumption() {
        let reader = SysfsBatteryEffectiveReader::from_native_path("BAT2").unwrap();
        assert!(reader.path.ends_with("BAT2/charge_control_end_threshold"));
        assert!(SysfsBatteryEffectiveReader::from_native_path("../BAT1").is_err());
    }

    #[test]
    fn production_contract_is_typed_and_not_shell_based() {
        assert_eq!(ASUSD_BUS_NAME, "xyz.ljones.Asusd");
        assert_eq!(ASUSD_OBJECT_PATH, "/xyz/ljones");
        assert_eq!(ASUSD_INTERFACE, "xyz.ljones.Platform");
        assert_eq!(EFFECTIVE_THRESHOLD_FILE, "charge_control_end_threshold");
    }
}
