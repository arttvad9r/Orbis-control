//! Internal ASUS Panel Overdrive mutation backend.
//!
//! Следует паттерну Battery (ADR 0007): единственный writer для
//! `panel_overdrive` — asusd (он персистит значение в своём конфиге и
//! восстанавливает его при старте), поэтому mutation идёт через typed asusd
//! D-Bus setter, а authoritative read-back — через kernel sysfs
//! `panel_overdrive/current_value` (fresh read). Прямой sysfs write при
//! активном asusd запрещён как competing owner (аналогично ADR 0011).
//!
//! Evidence:
//! - kernel `drivers/platform/x86/asus-armoury.c`:
//!   `ASUS_ATTR_GROUP_BOOL_RW(panel_od, "panel_overdrive", ASUS_WMI_DEVID_PANEL_OD, ...)`;
//!   `current_value` = bool `0`/`1`, read через WMI DSTS;
//! - asusd `asusd/src/asus_armoury.rs`: interface `xyz.ljones.AsusArmoury`,
//!   object path `/xyz/ljones/asus_armoury/panel_overdrive`, writable property
//!   `current_value` (i32 `0`/`1`), тип `FirmwareAttributeType::Immediate`
//!   (persist + restore at startup).

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use orbis_core::action::ApplyResult;
use orbis_providers::error::ProviderError;
use zbus::Connection;

use crate::{AuthorizeError, Authorizer, provider_error_to_dbus};

/// asusd D-Bus bus name.
pub const ASUSD_BUS_NAME: &str = "xyz.ljones.Asusd";
/// asusd D-Bus armoury attribute object path for `panel_overdrive`.
pub const ASUSD_ARMOURY_OBJECT_PATH: &str = "/xyz/ljones/asus_armoury/panel_overdrive";
/// asusd D-Bus armoury attribute interface.
pub const ASUSD_ARMOURY_INTERFACE: &str = "xyz.ljones.AsusArmoury";
/// Kernel sysfs authoritative current value path.
pub const PANEL_OVERDRIVE_CURRENT_VALUE_PATH: &str =
    "/sys/class/firmware-attributes/asus-armoury/attributes/panel_overdrive/current_value";

/// Strict decode of a raw asusd/backend value into the bool wire.
///
/// Kernel contract is `0`/`1`; anything else is malformed backend evidence and
/// must not become a fake default.
pub fn panel_overdrive_from_backend(raw: i32) -> Result<bool, ProviderError> {
    match raw {
        0 => Ok(false),
        1 => Ok(true),
        other => Err(ProviderError::Internal(format!(
            "asus-armoury panel_overdrive unexpected backend value: {other}"
        ))),
    }
}

/// Typed asusd operations required by the mutation algorithm.
#[async_trait]
pub trait AsusdPanelOverdriveClient: Send + Sync {
    /// Set the `current_value` property (writes kernel sysfs through asusd).
    async fn set_current_value(&self, value: i32) -> Result<(), ProviderError>;
    /// Read the `current_value` property (fresh; reads kernel sysfs).
    async fn current_value(&self) -> Result<i32, ProviderError>;
}

/// Fresh read-only authoritative kernel state source.
#[async_trait]
pub trait PanelOverdriveReader: Send + Sync {
    /// Read the authoritative `0`/`1` state.
    async fn read_current_value(&self) -> Result<u8, ProviderError>;
}

/// Read-only `/sys/class/firmware-attributes/asus-armoury/attributes/
/// panel_overdrive/current_value` reader.
#[derive(Debug)]
pub struct SysfsPanelOverdriveReader {
    path: PathBuf,
}

impl Default for SysfsPanelOverdriveReader {
    fn default() -> Self {
        Self {
            path: PathBuf::from(PANEL_OVERDRIVE_CURRENT_VALUE_PATH),
        }
    }
}

impl SysfsPanelOverdriveReader {
    fn from_path(path: PathBuf) -> Self {
        Self { path }
    }

    #[cfg(test)]
    fn for_test(path: PathBuf) -> Self {
        Self::from_path(path)
    }
}

/// Map a read error on the fixed attribute path into the typed provider error.
///
/// `NotFound` proves structural absence (attribute not created by the driver),
/// `PermissionDenied` is preserved distinctly, every other I/O failure stays an
/// I/O error.
fn map_read_error(path: &Path, error: std::io::Error) -> ProviderError {
    match error.kind() {
        std::io::ErrorKind::NotFound => ProviderError::Unsupported(format!(
            "asus-armoury panel_overdrive attribute absent: {}",
            path.display()
        )),
        std::io::ErrorKind::PermissionDenied => ProviderError::PermissionDenied(format!(
            "asus-armoury panel_overdrive read denied: {}",
            path.display()
        )),
        _ => ProviderError::Io(error),
    }
}

#[async_trait]
impl PanelOverdriveReader for SysfsPanelOverdriveReader {
    async fn read_current_value(&self) -> Result<u8, ProviderError> {
        let raw = std::fs::read_to_string(&self.path)
            .map_err(|error| map_read_error(&self.path, error))?;
        match raw.trim() {
            "0" => Ok(0),
            "1" => Ok(1),
            other => Err(ProviderError::Internal(format!(
                "asus-armoury panel_overdrive malformed value: {other:?}"
            ))),
        }
    }
}

/// Discover a readable authoritative panel overdrive attribute.
///
/// Read-only discovery: only checks that the fixed kernel ABI file is
/// readable. Never writes and never mutates hardware. Content validation stays
/// at read-back time.
pub fn discover_panel_overdrive_reader_at(
    path: PathBuf,
) -> Result<SysfsPanelOverdriveReader, ProviderError> {
    std::fs::read_to_string(&path).map_err(|error| map_read_error(&path, error))?;
    Ok(SysfsPanelOverdriveReader::from_path(path))
}

/// Discover the production authoritative attribute (fixed kernel path).
pub fn discover_panel_overdrive_reader() -> Result<SysfsPanelOverdriveReader, ProviderError> {
    discover_panel_overdrive_reader_at(PathBuf::from(PANEL_OVERDRIVE_CURRENT_VALUE_PATH))
}

#[zbus::proxy(
    interface = "xyz.ljones.AsusArmoury",
    default_service = "xyz.ljones.Asusd",
    default_path = "/xyz/ljones/asus_armoury/panel_overdrive"
)]
trait AsusdArmouryAttribute {
    #[zbus(property)]
    fn current_value(&self) -> zbus::Result<i32>;

    #[zbus(property)]
    fn set_current_value(&self, value: i32) -> zbus::Result<()>;
}

/// Production typed client for the asusd compatibility backend.
pub struct ZbusAsusdPanelOverdriveClient {
    connection: Connection,
}

impl ZbusAsusdPanelOverdriveClient {
    /// Construct without performing a D-Bus call.
    pub fn new(connection: Connection) -> Self {
        Self { connection }
    }

    async fn proxy(&self) -> Result<AsusdArmouryAttributeProxy<'_>, ProviderError> {
        AsusdArmouryAttributeProxy::new(&self.connection)
            .await
            .map_err(|error| ProviderError::Dbus(format!("asusd armoury proxy: {error}")))
    }
}

#[async_trait]
impl AsusdPanelOverdriveClient for ZbusAsusdPanelOverdriveClient {
    async fn set_current_value(&self, value: i32) -> Result<(), ProviderError> {
        self.proxy()
            .await?
            .set_current_value(value)
            .await
            .map_err(|error| ProviderError::Dbus(format!("asusd panel_overdrive setter: {error}")))
    }

    async fn current_value(&self) -> Result<i32, ProviderError> {
        self.proxy()
            .await?
            .current_value()
            .await
            .map_err(|error| ProviderError::Dbus(format!("asusd panel_overdrive read: {error}")))
    }
}

/// Fresh result of a panel overdrive mutation and its authoritative read-back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelOverdriveMutationReadback {
    /// Requested wire value (`0`/`1`).
    pub requested: u8,
    /// Fresh authoritative observed value (`0`/`1`).
    pub observed: u8,
    /// `Applied` only when observed == requested.
    pub result: ApplyResult,
}

/// Typed runtime evidence for Panel Overdrive mutation backend availability.
///
/// Preserves the distinction between a proven backend (`Supported`), a
/// structurally absent mutation capability (`Unsupported`), a temporary
/// discovery failure (`TemporarilyUnavailable`), an authorization failure
/// (`PermissionDenied`) and missing evidence (`Unknown`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelOverdriveMutationStatus {
    /// A proven production mutation backend (authoritative reader + typed
    /// asusd client) is installed.
    Supported,
    /// Mutation capability is structurally absent (no `panel_overdrive` ABI).
    Unsupported,
    /// A known/expected backend is temporarily unavailable.
    TemporarilyUnavailable,
    /// Mutation exists but current authorization evidence denies it.
    PermissionDenied,
    /// No evidence about mutation availability.
    Unknown,
}

/// Stable wire values for `Hardware1.PanelOverdriveMutationStatus`.
///
/// The numeric values match the Battery/Performance mutation status wire
/// contract (identical semantic classes); each backend module keeps its own
/// named constants so the D-Bus contract stays self-contained.
pub mod panel_mutation_wire {
    use super::PanelOverdriveMutationStatus;

    /// Proven mutation backend / ABI present.
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
    pub fn to_wire(status: PanelOverdriveMutationStatus) -> u8 {
        match status {
            PanelOverdriveMutationStatus::Supported => SUPPORTED,
            PanelOverdriveMutationStatus::Unsupported => UNSUPPORTED,
            PanelOverdriveMutationStatus::TemporarilyUnavailable => TEMPORARILY_UNAVAILABLE,
            PanelOverdriveMutationStatus::PermissionDenied => PERMISSION_DENIED,
            PanelOverdriveMutationStatus::Unknown => UNKNOWN,
        }
    }

    /// Decode a wire value; unknown values produce `None` so callers classify
    /// them as `Unknown` instead of inventing a known state.
    pub fn from_wire(raw: u8) -> Option<PanelOverdriveMutationStatus> {
        match raw {
            SUPPORTED => Some(PanelOverdriveMutationStatus::Supported),
            UNSUPPORTED => Some(PanelOverdriveMutationStatus::Unsupported),
            TEMPORARILY_UNAVAILABLE => Some(PanelOverdriveMutationStatus::TemporarilyUnavailable),
            PERMISSION_DENIED => Some(PanelOverdriveMutationStatus::PermissionDenied),
            UNKNOWN => Some(PanelOverdriveMutationStatus::Unknown),
            _ => None,
        }
    }
}

#[async_trait]
pub trait PanelOverdriveMutationBackend: Send + Sync {
    /// Perform exactly one mutation with authoritative read-back.
    async fn set_panel_overdrive(
        &self,
        enabled: bool,
    ) -> Result<PanelOverdriveMutationReadback, ProviderError>;

    /// Report the typed runtime availability of this mutation backend.
    ///
    /// This is read-only evidence used by capability probing; it never
    /// performs I/O and never mutates hardware.
    fn mutation_status(&self) -> PanelOverdriveMutationStatus;
}

/// Internal compatibility backend; it never writes the kernel directly.
pub struct AsusdPanelOverdriveMutationBackend<A, R> {
    asusd: A,
    reader: R,
}

impl<A, R> AsusdPanelOverdriveMutationBackend<A, R> {
    /// Construct without performing I/O.
    pub fn new(asusd: A, reader: R) -> Self {
        Self { asusd, reader }
    }
}

impl<A, R> AsusdPanelOverdriveMutationBackend<A, R>
where
    A: AsusdPanelOverdriveClient,
    R: PanelOverdriveReader,
{
    /// Validate, perform one asusd setter, then perform a fresh read-back.
    pub async fn set_panel_overdrive(
        &self,
        enabled: bool,
    ) -> Result<PanelOverdriveMutationReadback, ProviderError> {
        let requested = if enabled { 1 } else { 0 };

        // Exactly one mutation, owned by asusd. No retry and no sysfs fallback.
        self.asusd.set_current_value(i32::from(requested)).await?;

        let observed = self.reader.read_current_value().await?;

        if observed != requested {
            return Err(ProviderError::BackendUnavailable(format!(
                "asus-armoury panel_overdrive read-back mismatch: expected={requested}, got={observed}"
            )));
        }

        Ok(PanelOverdriveMutationReadback {
            requested,
            observed,
            result: ApplyResult::Applied,
        })
    }
}

#[async_trait]
impl<A, R> PanelOverdriveMutationBackend for AsusdPanelOverdriveMutationBackend<A, R>
where
    A: AsusdPanelOverdriveClient,
    R: PanelOverdriveReader,
{
    async fn set_panel_overdrive(
        &self,
        enabled: bool,
    ) -> Result<PanelOverdriveMutationReadback, ProviderError> {
        self.set_panel_overdrive(enabled).await
    }

    fn mutation_status(&self) -> PanelOverdriveMutationStatus {
        PanelOverdriveMutationStatus::Supported
    }
}

/// Обработка Panel Overdrive mutation до публичного D-Bus boundary.
///
/// Декодирование wire-значения (`bool`) выполняется до authorization; D-Bus
/// `bool` уже строгий тип, поэтому malformed вход невозможен на этом уровне.
/// После authorization — ровно одна typed mutation + authoritative read-back;
/// результат `Applied` только при совпадении observed == requested.
pub async fn handle_set_panel_overdrive(
    authorizer: &dyn Authorizer,
    backend: &dyn PanelOverdriveMutationBackend,
    enabled: bool,
    sender: &str,
) -> zbus::fdo::Result<u8> {
    tracing::info!(enabled, "panel_overdrive Hardware1 request");
    authorizer.authorize(sender).await.map_err(|e| match e {
        AuthorizeError::Denied(msg) => zbus::fdo::Error::AccessDenied(msg),
        AuthorizeError::Failed(msg) => zbus::fdo::Error::Failed(msg),
    })?;

    let readback = backend
        .set_panel_overdrive(enabled)
        .await
        .map_err(provider_error_to_dbus)?;
    match readback.result {
        ApplyResult::Applied => Ok(readback.observed),
        other => Err(zbus::fdo::Error::Failed(format!(
            "hardwared: panel overdrive operation not confirmed: {other:?}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;
    use crate::{AuthorizeError, Authorizer};

    #[derive(Clone)]
    struct FakeAsusd {
        current: Arc<Mutex<i32>>,
        setter_calls: Arc<AtomicUsize>,
        setter_error: Arc<Mutex<Option<String>>>,
        getter_error: Arc<Mutex<Option<String>>>,
        getter_override: Arc<Mutex<Option<i32>>>,
    }

    impl FakeAsusd {
        fn new(current: i32) -> Self {
            Self {
                current: Arc::new(Mutex::new(current)),
                setter_calls: Arc::new(AtomicUsize::new(0)),
                setter_error: Arc::new(Mutex::new(None)),
                getter_error: Arc::new(Mutex::new(None)),
                getter_override: Arc::new(Mutex::new(None)),
            }
        }

        fn setter_calls(&self) -> usize {
            self.setter_calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl AsusdPanelOverdriveClient for FakeAsusd {
        async fn set_current_value(&self, value: i32) -> Result<(), ProviderError> {
            self.setter_calls.fetch_add(1, Ordering::SeqCst);
            if let Some(error) = self.setter_error.lock().unwrap().clone() {
                return Err(ProviderError::Dbus(error));
            }
            *self.current.lock().unwrap() = value;
            Ok(())
        }

        async fn current_value(&self) -> Result<i32, ProviderError> {
            if let Some(error) = self.getter_error.lock().unwrap().clone() {
                return Err(ProviderError::Dbus(error));
            }
            Ok(self
                .getter_override
                .lock()
                .unwrap()
                .unwrap_or(*self.current.lock().unwrap()))
        }
    }

    #[derive(Clone)]
    struct FakeReader {
        value: Arc<Mutex<Result<u8, String>>>,
        reads: Arc<AtomicUsize>,
    }

    impl FakeReader {
        fn new(value: Result<u8, String>) -> Self {
            Self {
                value: Arc::new(Mutex::new(value)),
                reads: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    #[async_trait]
    impl PanelOverdriveReader for FakeReader {
        async fn read_current_value(&self) -> Result<u8, ProviderError> {
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
        reader: FakeReader,
    ) -> AsusdPanelOverdriveMutationBackend<FakeAsusd, FakeReader> {
        AsusdPanelOverdriveMutationBackend::new(asusd, reader)
    }

    #[tokio::test]
    async fn setter_success_readback_match_is_applied() {
        let asusd = FakeAsusd::new(0);
        let result = backend(asusd.clone(), FakeReader::new(Ok(1)))
            .set_panel_overdrive(true)
            .await
            .unwrap();
        assert_eq!(asusd.setter_calls(), 1);
        assert_eq!(result.requested, 1);
        assert_eq!(result.observed, 1);
        assert_eq!(result.result, ApplyResult::Applied);
    }

    #[tokio::test]
    async fn setter_success_disable_readback_match_is_applied() {
        let asusd = FakeAsusd::new(1);
        let result = backend(asusd.clone(), FakeReader::new(Ok(0)))
            .set_panel_overdrive(false)
            .await
            .unwrap();
        assert_eq!(asusd.setter_calls(), 1);
        assert_eq!(result.requested, 0);
        assert_eq!(result.observed, 0);
        assert_eq!(result.result, ApplyResult::Applied);
    }

    #[tokio::test]
    async fn setter_success_readback_mismatch_is_error_without_retry() {
        let asusd = FakeAsusd::new(0);
        let result = backend(asusd.clone(), FakeReader::new(Ok(0)))
            .set_panel_overdrive(true)
            .await;
        assert!(matches!(result, Err(ProviderError::BackendUnavailable(_))));
        assert_eq!(asusd.setter_calls(), 1);
    }

    #[tokio::test]
    async fn setter_success_readback_unavailable_is_not_optimistic_success() {
        let asusd = FakeAsusd::new(0);
        let reader = FakeReader::new(Err("read-back unavailable".into()));
        let result = backend(asusd.clone(), reader.clone())
            .set_panel_overdrive(true)
            .await;
        assert!(matches!(result, Err(ProviderError::Dbus(_))));
        assert_eq!(asusd.setter_calls(), 1);
        assert_eq!(reader.reads.load(Ordering::SeqCst), 1);
        assert!(!matches!(result, Ok(readback) if readback.result == ApplyResult::Applied));
    }

    #[tokio::test]
    async fn setter_error_is_propagated_without_reader_call() {
        let asusd = FakeAsusd::new(0);
        *asusd.setter_error.lock().unwrap() = Some("setter failed".into());
        let reader = FakeReader::new(Ok(1));
        let result = backend(asusd.clone(), reader.clone())
            .set_panel_overdrive(true)
            .await;
        assert!(matches!(result, Err(ProviderError::Dbus(_))));
        assert_eq!(asusd.setter_calls(), 1);
        assert_eq!(reader.reads.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn asusd_getter_error_is_propagated() {
        let asusd = FakeAsusd::new(0);
        *asusd.getter_error.lock().unwrap() = Some("getter failed".into());
        let error = asusd.current_value().await.expect_err("getter error");
        assert!(matches!(error, ProviderError::Dbus(_)));
    }

    #[tokio::test]
    async fn sysfs_reader_is_fresh_and_read_only() {
        let directory =
            std::env::temp_dir().join(format!("orbis-hardwared-panel-{}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("current_value");
        std::fs::write(&path, "0\n").unwrap();
        let reader = SysfsPanelOverdriveReader::for_test(path.clone());
        assert_eq!(reader.read_current_value().await.unwrap(), 0);
        std::fs::write(&path, "1\n").unwrap();
        assert_eq!(reader.read_current_value().await.unwrap(), 1);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[tokio::test]
    async fn sysfs_reader_malformed_is_internal_not_fake_default() {
        for malformed in ["2\n", "true\n", "\n", ""] {
            let directory = std::env::temp_dir().join(format!(
                "orbis-hardwared-panel-malformed-{}-{:?}",
                std::process::id(),
                malformed.len()
            ));
            std::fs::create_dir(&directory).unwrap();
            let path = directory.join("current_value");
            std::fs::write(&path, malformed).unwrap();
            let reader = SysfsPanelOverdriveReader::for_test(path.clone());
            let error = reader
                .read_current_value()
                .await
                .expect_err("malformed must fail");
            assert!(
                matches!(error, ProviderError::Internal(_)),
                "malformed {malformed:?} must be Internal, got {error:?}"
            );
            std::fs::remove_file(path).unwrap();
            std::fs::remove_dir(directory).unwrap();
        }
    }

    #[tokio::test]
    async fn sysfs_reader_missing_attribute_is_unsupported() {
        let directory = std::env::temp_dir().join(format!(
            "orbis-hardwared-panel-missing-{}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("current_value");
        let reader = SysfsPanelOverdriveReader::for_test(path.clone());
        let error = reader.read_current_value().await.expect_err("absent");
        assert!(matches!(error, ProviderError::Unsupported(_)));
        std::fs::remove_dir(directory).unwrap();
    }

    #[tokio::test]
    async fn discovery_is_read_only_and_classifies_absence() {
        let directory = std::env::temp_dir().join(format!(
            "orbis-hardwared-panel-discovery-{}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("current_value");

        // Absent attribute → Unsupported (structural absence, not fake OK).
        let error = discover_panel_overdrive_reader_at(path.clone()).expect_err("absent");
        assert!(matches!(error, ProviderError::Unsupported(_)));

        // Present readable attribute → reader, without any write.
        std::fs::write(&path, "0\n").unwrap();
        let reader = discover_panel_overdrive_reader_at(path.clone()).unwrap();
        assert_eq!(reader.read_current_value().await.unwrap(), 0);

        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn backend_wire_decode_strict() {
        assert!(!panel_overdrive_from_backend(0).unwrap());
        assert!(panel_overdrive_from_backend(1).unwrap());
        for other in [-1, 2, 5, 100] {
            assert!(matches!(
                panel_overdrive_from_backend(other),
                Err(ProviderError::Internal(_))
            ));
        }
    }

    #[test]
    fn panel_mutation_wire_roundtrip_is_total() {
        for status in [
            PanelOverdriveMutationStatus::Supported,
            PanelOverdriveMutationStatus::Unsupported,
            PanelOverdriveMutationStatus::TemporarilyUnavailable,
            PanelOverdriveMutationStatus::PermissionDenied,
            PanelOverdriveMutationStatus::Unknown,
        ] {
            let wire = panel_mutation_wire::to_wire(status);
            assert_eq!(panel_mutation_wire::from_wire(wire), Some(status));
        }
        assert_eq!(panel_mutation_wire::from_wire(99), None);
    }

    #[test]
    fn production_contract_is_typed_and_not_shell_based() {
        assert_eq!(ASUSD_BUS_NAME, "xyz.ljones.Asusd");
        assert_eq!(
            ASUSD_ARMOURY_OBJECT_PATH,
            "/xyz/ljones/asus_armoury/panel_overdrive"
        );
        assert_eq!(ASUSD_ARMOURY_INTERFACE, "xyz.ljones.AsusArmoury");
        assert_eq!(
            PANEL_OVERDRIVE_CURRENT_VALUE_PATH,
            "/sys/class/firmware-attributes/asus-armoury/attributes/panel_overdrive/current_value"
        );
    }

    #[derive(Clone, Copy)]
    enum AuthOutcome {
        Ok,
        Denied,
        Failed,
    }

    struct FakeAuthorizer {
        outcome: Arc<Mutex<AuthOutcome>>,
        calls: Arc<AtomicUsize>,
        last_sender: Arc<Mutex<Option<String>>>,
    }

    impl FakeAuthorizer {
        fn new(outcome: AuthOutcome) -> Self {
            Self {
                outcome: Arc::new(Mutex::new(outcome)),
                calls: Arc::new(AtomicUsize::new(0)),
                last_sender: Arc::new(Mutex::new(None)),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl Authorizer for FakeAuthorizer {
        async fn authorize(&self, sender: &str) -> Result<(), AuthorizeError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.last_sender.lock().unwrap() = Some(sender.to_string());
            match *self.outcome.lock().unwrap() {
                AuthOutcome::Ok => Ok(()),
                AuthOutcome::Denied => Err(AuthorizeError::Denied("denied".into())),
                AuthOutcome::Failed => Err(AuthorizeError::Failed("polkit down".into())),
            }
        }
    }

    #[tokio::test]
    async fn authorization_denied_means_backend_not_called() {
        let asusd = FakeAsusd::new(0);
        let reader = FakeReader::new(Ok(1));
        let backend = backend(asusd.clone(), reader.clone());
        let auth = FakeAuthorizer::new(AuthOutcome::Denied);
        let error = handle_set_panel_overdrive(&auth, &backend, true, ":1.42")
            .await
            .expect_err("denied");
        assert!(matches!(error, zbus::fdo::Error::AccessDenied(_)));
        assert_eq!(asusd.setter_calls(), 0);
        assert_eq!(reader.reads.load(Ordering::SeqCst), 0);
        assert_eq!(auth.calls(), 1);
        assert_eq!(auth.last_sender.lock().unwrap().as_deref(), Some(":1.42"));
    }

    #[tokio::test]
    async fn authorization_failed_means_backend_not_called() {
        let asusd = FakeAsusd::new(0);
        let reader = FakeReader::new(Ok(1));
        let backend = backend(asusd.clone(), reader.clone());
        let auth = FakeAuthorizer::new(AuthOutcome::Failed);
        let error = handle_set_panel_overdrive(&auth, &backend, true, ":1.7")
            .await
            .expect_err("polkit down");
        assert!(matches!(error, zbus::fdo::Error::Failed(_)));
        assert_eq!(asusd.setter_calls(), 0);
        assert_eq!(reader.reads.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn authorized_returns_confirmed_observed_value() {
        let asusd = FakeAsusd::new(0);
        let reader = FakeReader::new(Ok(1));
        let backend = backend(asusd.clone(), reader.clone());
        let auth = FakeAuthorizer::new(AuthOutcome::Ok);
        let observed = handle_set_panel_overdrive(&auth, &backend, true, ":1.42")
            .await
            .expect("authorized");
        assert_eq!(observed, 1);
        assert_eq!(asusd.setter_calls(), 1);
    }

    #[tokio::test]
    async fn readback_mismatch_through_handler_is_failure() {
        let asusd = FakeAsusd::new(0);
        let reader = FakeReader::new(Ok(0));
        let backend = backend(asusd.clone(), reader.clone());
        let auth = FakeAuthorizer::new(AuthOutcome::Ok);
        let error = handle_set_panel_overdrive(&auth, &backend, true, ":1.42")
            .await
            .expect_err("mismatch");
        assert!(matches!(error, zbus::fdo::Error::Failed(_)));
        assert_eq!(asusd.setter_calls(), 1);
    }

    #[tokio::test]
    async fn unsupported_backend_propagates_not_supported() {
        struct UnsupportedBackend;

        #[async_trait]
        impl PanelOverdriveMutationBackend for UnsupportedBackend {
            async fn set_panel_overdrive(
                &self,
                _enabled: bool,
            ) -> Result<PanelOverdriveMutationReadback, ProviderError> {
                Err(ProviderError::Unsupported(
                    "panel overdrive ABI absent".into(),
                ))
            }

            fn mutation_status(&self) -> PanelOverdriveMutationStatus {
                PanelOverdriveMutationStatus::Unsupported
            }
        }

        let auth = FakeAuthorizer::new(AuthOutcome::Ok);
        let error = handle_set_panel_overdrive(&auth, &UnsupportedBackend, true, ":1.42")
            .await
            .expect_err("unsupported");
        assert!(matches!(error, zbus::fdo::Error::NotSupported(_)));
    }
}
