//! # orbis-hardwared — foundation привилегированного hardware helper.
//!
//! Единственная capability на этом этапе (ADR 0006):
//! `SetPerformanceProfile` через kernel `/sys/firmware/acpi/platform_profile`.
//!
//! - closed API: принимает только semantic [`PerformanceProfile`]; клиент не
//!   передаёт пути, строки или sysfs-символы;
//! - mapping внутри hardwared: `Silent → quiet`, `Balanced → balanced`,
//!   `Turbo → performance`;
//! - никакого generic filesystem writer API;
//! - никакого кэша/optimistic success: каждый вызов читает choices и делает
//!   fresh read-back;
//! - D-Bus/Hardware1 and polkit authorization are narrow typed boundaries;
//!   this crate does not perform real sysfs I/O in tests (fake backend).

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use orbis_core::action::ApplyResult;
use orbis_core::aura::AuraRgb;
use orbis_core::profile::PerformanceProfile;
use orbis_providers::asus_gpu_mode::{AsusGpuMode, ProductGpuOutcome};
use orbis_providers::error::ProviderError;
use orbis_providers::supergfxd::{SupergfxdMode, SupergfxdStagedState, SupergfxdUserAction};

pub mod asus_gpu_mode;
pub mod aura;
pub mod battery;
pub mod fans;
pub mod keyboard_backlight;
pub mod panel;
pub mod supergfxd;

use battery::{BatteryMutationBackend, BatteryMutationReadback};

/// Фиксированный production path kernel ABI: current profile.
pub const PLATFORM_PROFILE_PATH: &str = "/sys/firmware/acpi/platform_profile";
/// Фиксированный production path kernel ABI: доступные профили.
pub const PLATFORM_PROFILE_CHOICES_PATH: &str = "/sys/firmware/acpi/platform_profile_choices";

/// Exact kernel symbol для write-path (reverse read mapping, ADR 0006).
///
/// Total и закрытая функция: принимает только [`PerformanceProfile`]; никаких
/// строк/путей от клиента.
pub fn profile_symbol(profile: PerformanceProfile) -> &'static str {
    match profile {
        PerformanceProfile::Silent => "quiet",
        PerformanceProfile::Balanced => "balanced",
        PerformanceProfile::Turbo => "performance",
    }
}

/// Низкоуровневый файловый интерфейс (инъектируемый для тестов).
///
/// Это НЕ generic filesystem writer API: реализует только два read/write
/// метода, необходимых фиксированному writer-у.
pub trait ProfileIo: Send + Sync {
    /// Прочитать файл целиком (fresh; trim на стороне вызывающего).
    fn read_to_string(&self, path: &Path) -> Result<String, ProviderError>;
    /// Записать содержимое в файл (ровно один вызов на операцию).
    fn write(&self, path: &Path, content: &str) -> Result<(), ProviderError>;
}

/// Реальная реализация на `std::fs`.
pub struct StdProfileIo;

impl ProfileIo for StdProfileIo {
    fn read_to_string(&self, path: &Path) -> Result<String, ProviderError> {
        std::fs::read_to_string(path).map_err(ProviderError::Io)
    }

    fn write(&self, path: &Path, content: &str) -> Result<(), ProviderError> {
        std::fs::write(path, content).map_err(ProviderError::Io)
    }
}

/// Writer для kernel `platform_profile`.
///
/// Пути инъектируются в конструкторе (тесты — temp/fake); production default
/// фиксирован на `/sys/firmware/acpi/platform_profile(_choices)`.
///
/// Алгоритм [`set_performance_profile`](Self::set_performance_profile):
/// 1. прочитать choices fresh;
/// 2. убедиться, что requested symbol присутствует (иначе `Unsupported`);
/// 3. ровно один write requested symbol;
/// 4. fresh read-back current;
/// 5. success только если read-back совпал.
pub struct PlatformProfileWriter<S: ProfileIo> {
    io: S,
    profile_path: PathBuf,
    choices_path: PathBuf,
}

impl PlatformProfileWriter<StdProfileIo> {
    /// Создать writer над реальным `std::fs` с явными путями (test-only).
    ///
    /// Production public API использует только [`PlatformProfileWriter::default`]
    /// с фиксированными kernel paths; произвольные пути недоступны извне crate.
    pub(crate) fn new(profile_path: PathBuf, choices_path: PathBuf) -> Self {
        Self::with_io(StdProfileIo, profile_path, choices_path)
    }
}

impl Default for PlatformProfileWriter<StdProfileIo> {
    /// Production default: фиксированные kernel ABI paths.
    fn default() -> Self {
        Self::new(
            PathBuf::from(PLATFORM_PROFILE_PATH),
            PathBuf::from(PLATFORM_PROFILE_CHOICES_PATH),
        )
    }
}

impl<S: ProfileIo> PlatformProfileWriter<S> {
    /// Создать writer над инъектируемым IO (test-only; fake backend).
    ///
    /// Произвольные пути/IO недоступны извне crate: production использует
    /// только [`PlatformProfileWriter::default`] с фиксированными paths.
    pub(crate) fn with_io(io: S, profile_path: PathBuf, choices_path: PathBuf) -> Self {
        Self {
            io,
            profile_path,
            choices_path,
        }
    }

    fn read_trimmed(&self, path: &Path, what: &str) -> Result<String, ProviderError> {
        let raw = self.io.read_to_string(path)?;
        let trimmed = raw.trim().to_string();
        if trimmed.is_empty() {
            return Err(ProviderError::Internal(format!(
                "hardwared: backend state '{what}' пуст (malformed)"
            )));
        }
        Ok(trimmed)
    }

    /// Выполнить ровно одну Performance profile mutation с authoritative
    /// read-back. Оптимистичный success не используется.
    pub fn set_performance_profile(
        &self,
        profile: PerformanceProfile,
    ) -> Result<ApplyResult, ProviderError> {
        let symbol = profile_symbol(profile);

        let choices_raw = self.read_trimmed(&self.choices_path, "platform_profile_choices")?;
        let choices: Vec<&str> = choices_raw.split_whitespace().collect();
        if choices.is_empty() {
            return Err(ProviderError::Internal(
                "hardwared: platform_profile_choices не содержит ни одного символа".into(),
            ));
        }

        if !choices.contains(&symbol) {
            return Err(ProviderError::Unsupported(format!(
                "hardwared: profile symbol '{symbol}' отсутствует в platform_profile_choices"
            )));
        }

        self.io.write(&self.profile_path, &format!("{symbol}\n"))?;
        let current = self.read_trimmed(&self.profile_path, "platform_profile")?;

        if current != symbol {
            return Err(ProviderError::Conflict(format!(
                "hardwared: read-back не подтвердил requested profile: expected='{symbol}', got='{current}'"
            )));
        }

        Ok(ApplyResult::Applied)
    }

    /// Read-only typed evidence about Performance mutation backend availability.
    ///
    /// Performs a single fresh read of `platform_profile_choices` — the same
    /// read the mutation path performs — and classifies the result. Never
    /// writes and never mutates hardware.
    pub fn mutation_status(&self) -> PerformanceMutationStatus {
        match self.read_trimmed(&self.choices_path, "platform_profile_choices") {
            Ok(choices) if choices.split_whitespace().next().is_some() => {
                PerformanceMutationStatus::Supported
            }
            // Empty choices file is malformed — no usable backend evidence.
            Ok(_) => PerformanceMutationStatus::Unknown,
            Err(ProviderError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                PerformanceMutationStatus::Unsupported
            }
            Err(ProviderError::Io(error))
                if error.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                PerformanceMutationStatus::PermissionDenied
            }
            Err(ProviderError::Io(_)) => PerformanceMutationStatus::TemporarilyUnavailable,
            // Empty/malformed content is reported by read_trimmed as Internal.
            Err(_) => PerformanceMutationStatus::Unknown,
        }
    }
}

// ---------------------------------------------------------------------------
// D-Bus service boundary (system bus)
// ---------------------------------------------------------------------------

/// Стабильный D-Bus name system service.
pub const DBUS_NAME: &str = "io.github.orbiscontrol.Hardware";
/// Стабильный object path system service.
pub const DBUS_OBJECT_PATH: &str = "/io/github/orbiscontrol/Hardware";
/// Имя интерфейса D-Bus (версия интерфейса как `1`).
pub const DBUS_INTERFACE_NAME: &str = "io.github.orbiscontrol.Hardware1";
/// Polkit action id для Performance mutation.
pub const POLKIT_ACTION: &str = "io.github.orbiscontrol.hardware.set-performance-profile";
/// Polkit action id for Battery charge-limit mutation.
pub const BATTERY_POLKIT_ACTION: &str = "io.github.orbiscontrol.hardware.set-charge-limit";
/// Polkit action id for the injectable GPU mutation boundary.
pub const GPU_POLKIT_ACTION: &str = "io.github.orbiscontrol.hardware.set-gpu-mode";
/// Polkit action id for fan curve mutation, including profile-wide factory reset.
pub const FAN_POLKIT_ACTION: &str = "io.github.orbiscontrol.hardware.set-fan-curve";
/// Polkit action id for Panel Overdrive mutation (separate display-panel
/// capability/security domain; mirrors per-capability action pattern).
pub const PANEL_POLKIT_ACTION: &str = "io.github.orbiscontrol.hardware.set-panel-overdrive";
/// Polkit action id for keyboard backlight brightness mutation.
pub const KEYBOARD_BACKLIGHT_POLKIT_ACTION: &str =
    "io.github.orbiscontrol.hardware.set-keyboard-backlight";
/// Polkit action id for Aura Static RGB mutation (separate RGB capability/
/// security domain; mirrors per-capability action pattern).
pub const AURA_POLKIT_ACTION: &str = "io.github.orbiscontrol.hardware.set-aura-static-rgb";
/// Polkit action id for ASUS Armoury product GPU mode queueing.
pub const PRODUCT_GPU_POLKIT_ACTION: &str = "io.github.orbiscontrol.hardware.set-product-gpu-mode";

/// Wire-значения Performance profile (закрытый enum, никаких строк/путей).
pub mod wire {
    /// Silent.
    pub const SILENT: u8 = 0;
    /// Balanced.
    pub const BALANCED: u8 = 1;
    /// Turbo.
    pub const TURBO: u8 = 2;
}

/// Strict wire decode (0/1/2); неизвестное значение → `InvalidRequest`.
pub fn profile_from_wire(raw: u8) -> Result<PerformanceProfile, ProviderError> {
    match raw {
        wire::SILENT => Ok(PerformanceProfile::Silent),
        wire::BALANCED => Ok(PerformanceProfile::Balanced),
        wire::TURBO => Ok(PerformanceProfile::Turbo),
        other => Err(ProviderError::InvalidRequest(format!(
            "hardwared: неизвестный performance wire value {other}"
        ))),
    }
}

/// Wire encode подтверждённого профиля.
pub fn profile_to_wire(profile: PerformanceProfile) -> u8 {
    match profile {
        PerformanceProfile::Silent => wire::SILENT,
        PerformanceProfile::Balanced => wire::BALANCED,
        PerformanceProfile::Turbo => wire::TURBO,
    }
}

/// Typed runtime evidence for Performance mutation backend availability.
///
/// Unlike the Battery backend, the Performance writer is always constructed;
/// its real availability is proven by a read-only fresh read of
/// `platform_profile_choices` (the same read the writer performs at mutation
/// time). The classification preserves the distinction between a present ABI
/// (`Supported`), an absent ABI (`Unsupported`), a transient read failure
/// (`TemporarilyUnavailable`) and a permission failure (`PermissionDenied`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerformanceMutationStatus {
    /// The kernel `platform_profile` ABI is present and readable, so the
    /// mutation path exists. Operational write failures are not predicted.
    Supported,
    /// The `platform_profile` ABI is structurally absent on this device.
    Unsupported,
    /// A known/expected ABI is temporarily unreadable.
    TemporarilyUnavailable,
    /// The ABI exists but current read authorization denies access.
    PermissionDenied,
    /// No provable evidence about mutation availability.
    Unknown,
}

/// Stable wire values for `Hardware1.PerformanceMutationStatus`.
///
/// The numeric values intentionally match the Battery mutation status wire
/// contract (identical semantic classes); each backend module still keeps its
/// own named constants so the D-Bus contract stays self-contained.
pub mod performance_mutation_wire {
    use super::PerformanceMutationStatus;

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
    pub fn to_wire(status: PerformanceMutationStatus) -> u8 {
        match status {
            PerformanceMutationStatus::Supported => SUPPORTED,
            PerformanceMutationStatus::Unsupported => UNSUPPORTED,
            PerformanceMutationStatus::TemporarilyUnavailable => TEMPORARILY_UNAVAILABLE,
            PerformanceMutationStatus::PermissionDenied => PERMISSION_DENIED,
            PerformanceMutationStatus::Unknown => UNKNOWN,
        }
    }

    /// Decode a wire value; unknown values produce `None` so callers classify
    /// them as `Unknown` instead of inventing a known state.
    pub fn from_wire(raw: u8) -> Option<PerformanceMutationStatus> {
        match raw {
            SUPPORTED => Some(PerformanceMutationStatus::Supported),
            UNSUPPORTED => Some(PerformanceMutationStatus::Unsupported),
            TEMPORARILY_UNAVAILABLE => Some(PerformanceMutationStatus::TemporarilyUnavailable),
            PERMISSION_DENIED => Some(PerformanceMutationStatus::PermissionDenied),
            UNKNOWN => Some(PerformanceMutationStatus::Unknown),
            _ => None,
        }
    }
}

/// Ошибка авторизации.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorizeError {
    /// Не авторизован (deny/challenge/cancel) — клиенту `AccessDenied`.
    Denied(String),
    /// Ошибка проверки авторизации (polkit/transport) — клиенту `Failed`.
    Failed(String),
}

/// Узкая абстракция авторизации.
#[async_trait]
pub trait Authorizer: Send + Sync {
    /// Разрешить ли caller (unique sender name на system bus) операцию.
    async fn authorize(&self, sender: &str) -> Result<(), AuthorizeError>;
}

/// Production polkit authorizer.
pub struct PolkitAuthorizer {
    connection: zbus::Connection,
    action: &'static str,
}

impl PolkitAuthorizer {
    /// Создать authorizer над готовой system-bus Connection.
    pub fn new(connection: zbus::Connection) -> Self {
        Self {
            connection,
            action: POLKIT_ACTION,
        }
    }

    /// Создать authorizer для отдельной typed capability action.
    pub fn with_action(connection: zbus::Connection, action: &'static str) -> Self {
        Self { connection, action }
    }
}

#[async_trait]
impl Authorizer for PolkitAuthorizer {
    async fn authorize(&self, sender: &str) -> Result<(), AuthorizeError> {
        let proxy = zbus_polkit::policykit1::AuthorityProxy::new(&self.connection)
            .await
            .map_err(|e| AuthorizeError::Failed(format!("polkit proxy: {e}")))?;

        let mut subject_details = std::collections::HashMap::new();
        let name_value = zbus::zvariant::Value::from(sender.to_string());
        subject_details.insert(
            "name".to_string(),
            zbus::zvariant::OwnedValue::try_from(name_value)
                .map_err(|e| AuthorizeError::Failed(format!("polkit subject: {e}")))?,
        );
        let subject = zbus_polkit::policykit1::Subject {
            subject_kind: "system-bus-name".to_string(),
            subject_details,
        };

        let result = proxy
            .check_authorization(
                &subject,
                self.action,
                &std::collections::HashMap::new(),
                Default::default(),
                "",
            )
            .await
            .map_err(|e| AuthorizeError::Failed(format!("polkit check: {e}")))?;

        if result.is_authorized {
            Ok(())
        } else {
            Err(AuthorizeError::Denied(
                "polkit: операция не авторизована".into(),
            ))
        }
    }
}

fn provider_error_to_dbus(error: ProviderError) -> zbus::fdo::Error {
    match error {
        ProviderError::Unsupported(msg) => zbus::fdo::Error::NotSupported(msg),
        ProviderError::PermissionDenied(msg) => zbus::fdo::Error::AccessDenied(msg),
        ProviderError::InvalidRequest(msg) => zbus::fdo::Error::InvalidArgs(msg),
        ProviderError::BackendUnavailable(msg) => zbus::fdo::Error::Failed(msg),
        ProviderError::Timeout(msg) => zbus::fdo::Error::Failed(msg),
        ProviderError::Io(e) => zbus::fdo::Error::Failed(e.to_string()),
        ProviderError::Dbus(msg) => zbus::fdo::Error::Failed(msg),
        ProviderError::Internal(msg) => zbus::fdo::Error::Failed(msg),
        ProviderError::Conflict(msg) => zbus::fdo::Error::Failed(msg),
    }
}

pub async fn handle_set_performance_profile<A, S>(
    authorizer: &A,
    writer: &PlatformProfileWriter<S>,
    raw: u8,
    sender: &str,
) -> zbus::fdo::Result<u8>
where
    A: Authorizer + ?Sized,
    S: ProfileIo,
{
    let profile = profile_from_wire(raw).map_err(provider_error_to_dbus)?;
    authorizer.authorize(sender).await.map_err(|e| match e {
        AuthorizeError::Denied(msg) => zbus::fdo::Error::AccessDenied(msg),
        AuthorizeError::Failed(msg) => zbus::fdo::Error::Failed(msg),
    })?;
    let result = writer
        .set_performance_profile(profile)
        .map_err(provider_error_to_dbus)?;
    match result {
        ApplyResult::Applied => Ok(profile_to_wire(profile)),
        other => Err(zbus::fdo::Error::Failed(format!(
            "hardwared: операция не подтверждена: {other:?}"
        ))),
    }
}

/// Обработка Battery mutation до публичного D-Bus boundary.
pub async fn handle_set_charge_limit(
    authorizer: &dyn Authorizer,
    backend: &dyn BatteryMutationBackend,
    percent: u8,
    sender: &str,
) -> zbus::fdo::Result<u8> {
    battery::validate_charge_limit(percent).map_err(provider_error_to_dbus)?;
    tracing::info!(requested_percent = percent, "battery Hardware1 request");
    authorizer.authorize(sender).await.map_err(|e| match e {
        AuthorizeError::Denied(msg) => zbus::fdo::Error::AccessDenied(msg),
        AuthorizeError::Failed(msg) => zbus::fdo::Error::Failed(msg),
    })?;

    let readback: BatteryMutationReadback = backend
        .set_charge_limit(percent)
        .await
        .map_err(provider_error_to_dbus)?;
    match readback.result {
        ApplyResult::Applied => Ok(readback.configured_percent),
        other => Err(zbus::fdo::Error::Failed(format!(
            "hardwared: battery operation not confirmed: {other:?}"
        ))),
    }
}

/// Обработка fan curve mutation до публичного D-Bus boundary.
pub async fn handle_set_fan_curve<A>(
    authorizer: &A,
    backend: &dyn fans::FanCurveMutationOperation,
    profile_raw: u32,
    fan_raw: u8,
    curve_wire: &fans::FanCurveWire,
    sender: &str,
) -> zbus::fdo::Result<u32>
where
    A: Authorizer + ?Sized,
{
    let profile = fans::fan_profile_from_wire(profile_raw).map_err(provider_error_to_dbus)?;
    let fan = fans::fan_from_wire(fan_raw).map_err(provider_error_to_dbus)?;
    let curve = fans::fan_curve_from_wire(curve_wire).map_err(provider_error_to_dbus)?;
    fans::validate_fan_curve(&curve).map_err(provider_error_to_dbus)?;

    authorizer.authorize(sender).await.map_err(|e| match e {
        AuthorizeError::Denied(msg) => zbus::fdo::Error::AccessDenied(msg),
        AuthorizeError::Failed(msg) => zbus::fdo::Error::Failed(msg),
    })?;

    let readback = backend
        .set_fan_curve(profile, &fan, &curve)
        .await
        .map_err(provider_error_to_dbus)?;

    match readback.result {
        ApplyResult::Applied => Ok(fans::fan_profile_to_wire(profile)),
        other => Err(zbus::fdo::Error::Failed(format!(
            "hardwared: fan curve operation not confirmed: {other:?}"
        ))),
    }
}

/// Handle profile-wide platform factory fan-curve reset.
///
/// Profile decode happens before authorization/backend I/O. The same fan
/// mutation polkit action is reused because this is the same capability and
/// risk class as custom fan-curve writes. Success requires the backend's fresh
/// post-reset FanCurveData observation.
pub async fn handle_reset_fan_curves_to_defaults<A>(
    authorizer: &A,
    backend: &dyn fans::FanCurveMutationOperation,
    profile_raw: u32,
    sender: &str,
) -> zbus::fdo::Result<u32>
where
    A: Authorizer + ?Sized,
{
    let profile = fans::fan_profile_from_wire(profile_raw).map_err(provider_error_to_dbus)?;
    authorizer.authorize(sender).await.map_err(|e| match e {
        AuthorizeError::Denied(msg) => zbus::fdo::Error::AccessDenied(msg),
        AuthorizeError::Failed(msg) => zbus::fdo::Error::Failed(msg),
    })?;
    let readback = backend
        .reset_curves_to_defaults(profile)
        .await
        .map_err(provider_error_to_dbus)?;
    match readback.result {
        ApplyResult::Applied
            if readback.requested_profile == profile && readback.observed_curves > 0 =>
        {
            Ok(fans::fan_profile_to_wire(profile))
        }
        other => Err(zbus::fdo::Error::Failed(format!(
            "hardwared: fan factory reset not confirmed: result={other:?}, observed_curves={}",
            readback.observed_curves
        ))),
    }
}

/// Stable wire DTO for one staged backend-mode observation.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    zbus::zvariant::Type,
    zbus::zvariant::OwnedValue,
)]
pub struct GpuMutationResult {
    pub requested_mode: u32,
    pub returned_user_action: u32,
    pub current_mode: u32,
    pub pending_mode: u32,
    pub pending_user_action: u32,
    pub outcome: u32,
}

const GPU_OUTCOME_APPLIED: u32 = 0;
const GPU_OUTCOME_PENDING: u32 = 1;
const GPU_OUTCOME_REQUIRES_USER_ACTION: u32 = 2;
const GPU_OUTCOME_INCONSISTENT: u32 = 3;

fn backend_mode_from_wire(raw: u32) -> Result<SupergfxdMode, ProviderError> {
    match SupergfxdMode::from_wire(raw) {
        mode @ (SupergfxdMode::Hybrid
        | SupergfxdMode::Integrated
        | SupergfxdMode::NvidiaNoModeset
        | SupergfxdMode::Vfio
        | SupergfxdMode::AsusEgpu
        | SupergfxdMode::AsusMuxDgpu) => Ok(mode),
        SupergfxdMode::None | SupergfxdMode::Unknown(_) => Err(ProviderError::InvalidRequest(
            format!("unknown or invalid supergfxd GPU mode wire value {raw}"),
        )),
    }
}

fn backend_mode_to_wire(mode: SupergfxdMode) -> u32 {
    match mode {
        SupergfxdMode::Hybrid => 0,
        SupergfxdMode::Integrated => 1,
        SupergfxdMode::NvidiaNoModeset => 2,
        SupergfxdMode::Vfio => 3,
        SupergfxdMode::AsusEgpu => 4,
        SupergfxdMode::AsusMuxDgpu => 5,
        SupergfxdMode::None => 6,
        SupergfxdMode::Unknown(raw) => raw,
    }
}

fn user_action_to_wire(action: SupergfxdUserAction) -> u32 {
    match action {
        SupergfxdUserAction::Logout => 0,
        SupergfxdUserAction::Reboot => 1,
        SupergfxdUserAction::SwitchToIntegrated => 2,
        SupergfxdUserAction::AsusEgpuDisable => 3,
        SupergfxdUserAction::Nothing => 4,
        SupergfxdUserAction::Unknown(raw) => raw,
    }
}

fn staged_state_to_wire(state: SupergfxdStagedState) -> u32 {
    match state {
        SupergfxdStagedState::Applied => GPU_OUTCOME_APPLIED,
        SupergfxdStagedState::Pending => GPU_OUTCOME_PENDING,
        SupergfxdStagedState::RequiresUserAction(_) => GPU_OUTCOME_REQUIRES_USER_ACTION,
        SupergfxdStagedState::Inconsistent => GPU_OUTCOME_INCONSISTENT,
    }
}

pub async fn handle_set_gpu_mode(
    authorizer: &dyn Authorizer,
    backend: Option<&dyn supergfxd::SupergfxdMutationOperation>,
    raw: u32,
    sender: &str,
) -> zbus::fdo::Result<GpuMutationResult> {
    let requested = backend_mode_from_wire(raw).map_err(provider_error_to_dbus)?;
    authorizer
        .authorize(sender)
        .await
        .map_err(|error| match error {
            AuthorizeError::Denied(message) => zbus::fdo::Error::AccessDenied(message),
            AuthorizeError::Failed(message) => zbus::fdo::Error::Failed(message),
        })?;
    let backend =
        backend.ok_or_else(|| zbus::fdo::Error::NotSupported("GPU backend unavailable".into()))?;
    let observation = backend
        .request_mode(requested)
        .await
        .map_err(provider_error_to_dbus)?;
    Ok(GpuMutationResult {
        requested_mode: backend_mode_to_wire(observation.requested),
        returned_user_action: user_action_to_wire(observation.returned_action),
        current_mode: backend_mode_to_wire(observation.snapshot.current_mode),
        pending_mode: backend_mode_to_wire(observation.snapshot.pending_mode),
        pending_user_action: user_action_to_wire(observation.snapshot.pending_user_action),
        outcome: staged_state_to_wire(observation.state),
    })
}

/// Result of the ASUS Armoury product GPU queue operation.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, zbus::zvariant::Type,
)]
pub struct ProductGpuMutationResult {
    pub requested_mode: u32,
    pub current_mode: u32,
    pub queued_mode: u32,
    pub outcome: u32,
    pub reboot_required: bool,
}

pub async fn handle_set_product_gpu_mode(
    authorizer: &dyn Authorizer,
    backend: Option<&dyn asus_gpu_mode::AsusProductGpuMutationOperation>,
    raw: u32,
    sender: &str,
) -> zbus::fdo::Result<ProductGpuMutationResult> {
    let requested = match raw {
        0 => AsusGpuMode::Hybrid,
        1 => AsusGpuMode::Integrated,
        2 => AsusGpuMode::Ultimate,
        _ => {
            return Err(zbus::fdo::Error::InvalidArgs(format!(
                "unknown ASUS product GPU mode {raw}"
            )));
        }
    };
    authorizer
        .authorize(sender)
        .await
        .map_err(|error| match error {
            AuthorizeError::Denied(message) => zbus::fdo::Error::AccessDenied(message),
            AuthorizeError::Failed(message) => zbus::fdo::Error::Failed(message),
        })?;
    let backend = backend.ok_or_else(|| {
        zbus::fdo::Error::NotSupported("ASUS product GPU backend unavailable".into())
    })?;
    let result = backend
        .set_mode(requested)
        .await
        .map_err(provider_error_to_dbus)?;
    let queued_mode = result
        .snapshot
        .queued_mode
        .and_then(|mode| match mode {
            AsusGpuMode::Hybrid => Some(0),
            AsusGpuMode::Integrated => Some(1),
            AsusGpuMode::Ultimate => Some(2),
            _ => None,
        })
        .unwrap_or(u32::MAX);
    let outcome = match result.outcome {
        ProductGpuOutcome::AlreadyActive => 0,
        ProductGpuOutcome::RebootRequired => 1,
        ProductGpuOutcome::Unknown => 2,
        ProductGpuOutcome::Inconsistent => 3,
    };
    Ok(ProductGpuMutationResult {
        requested_mode: raw,
        current_mode: match result.snapshot.current_mode {
            AsusGpuMode::Hybrid => 0,
            AsusGpuMode::Integrated => 1,
            AsusGpuMode::Ultimate => 2,
            _ => u32::MAX,
        },
        queued_mode,
        outcome,
        reboot_required: result.snapshot.reboot_required(),
    })
}

/// Service object интерфейса `io.github.orbiscontrol.Hardware1`.
pub struct HardwareService {
    authorizer: Box<dyn Authorizer>,
    writer: PlatformProfileWriter<StdProfileIo>,
    battery_authorizer: Box<dyn Authorizer>,
    battery_backend: Option<Box<dyn BatteryMutationBackend>>,
    gpu_authorizer: Box<dyn Authorizer>,
    gpu_backend: Option<Box<dyn supergfxd::SupergfxdMutationOperation>>,
    gpu_sender_fallback: Option<String>,
    product_gpu_authorizer: Box<dyn Authorizer>,
    product_gpu_backend: Option<Box<dyn asus_gpu_mode::AsusProductGpuMutationOperation>>,
    fan_authorizer: Box<dyn Authorizer>,
    fan_backend: Option<Box<dyn fans::FanCurveMutationOperation>>,
    panel_authorizer: Box<dyn Authorizer>,
    panel_backend: Option<Box<dyn panel::PanelOverdriveMutationBackend>>,
    kb_authorizer: Box<dyn Authorizer>,
    kb_backend: Option<Box<dyn keyboard_backlight::KeyboardBacklightMutationBackend>>,
    aura_authorizer: Box<dyn Authorizer>,
    aura_backend: Option<Box<dyn aura::AuraStaticRgbMutationBackend>>,
}

impl HardwareService {
    pub fn new(authorizer: Box<dyn Authorizer>) -> Self {
        Self {
            authorizer,
            writer: PlatformProfileWriter::default(),
            battery_authorizer: Box::new(DisabledAuthorizer),
            battery_backend: None,
            gpu_authorizer: Box::new(DisabledAuthorizer),
            gpu_backend: None,
            gpu_sender_fallback: None,
            product_gpu_authorizer: Box::new(DisabledAuthorizer),
            product_gpu_backend: None,
            fan_authorizer: Box::new(DisabledAuthorizer),
            fan_backend: None,
            panel_authorizer: Box::new(DisabledAuthorizer),
            panel_backend: None,
            kb_authorizer: Box::new(DisabledAuthorizer),
            kb_backend: None,
            aura_authorizer: Box::new(DisabledAuthorizer),
            aura_backend: None,
        }
    }

    pub fn with_battery_backend(
        authorizer: Box<dyn Authorizer>,
        battery_backend: Box<dyn BatteryMutationBackend>,
        battery_authorizer: Box<dyn Authorizer>,
    ) -> Self {
        Self {
            authorizer,
            writer: PlatformProfileWriter::default(),
            battery_authorizer,
            battery_backend: Some(battery_backend),
            gpu_authorizer: Box::new(DisabledAuthorizer),
            gpu_backend: None,
            gpu_sender_fallback: None,
            product_gpu_authorizer: Box::new(DisabledAuthorizer),
            product_gpu_backend: None,
            fan_authorizer: Box::new(DisabledAuthorizer),
            fan_backend: None,
            panel_authorizer: Box::new(DisabledAuthorizer),
            panel_backend: None,
            kb_authorizer: Box::new(DisabledAuthorizer),
            kb_backend: None,
            aura_authorizer: Box::new(DisabledAuthorizer),
            aura_backend: None,
        }
    }

    pub fn with_battery_and_gpu_backends(
        authorizer: Box<dyn Authorizer>,
        battery_backend: Box<dyn BatteryMutationBackend>,
        battery_authorizer: Box<dyn Authorizer>,
        gpu_backend: Box<dyn supergfxd::SupergfxdMutationOperation>,
        gpu_authorizer: Box<dyn Authorizer>,
    ) -> Self {
        Self {
            authorizer,
            writer: PlatformProfileWriter::default(),
            battery_authorizer,
            battery_backend: Some(battery_backend),
            gpu_authorizer,
            gpu_backend: Some(gpu_backend),
            gpu_sender_fallback: None,
            product_gpu_authorizer: Box::new(DisabledAuthorizer),
            product_gpu_backend: None,
            fan_authorizer: Box::new(DisabledAuthorizer),
            fan_backend: None,
            panel_authorizer: Box::new(DisabledAuthorizer),
            panel_backend: None,
            kb_authorizer: Box::new(DisabledAuthorizer),
            kb_backend: None,
            aura_authorizer: Box::new(DisabledAuthorizer),
            aura_backend: None,
        }
    }

    pub fn with_fan_backend(
        authorizer: Box<dyn Authorizer>,
        fan_backend: Box<dyn fans::FanCurveMutationOperation>,
        fan_authorizer: Box<dyn Authorizer>,
    ) -> Self {
        Self {
            authorizer,
            writer: PlatformProfileWriter::default(),
            battery_authorizer: Box::new(DisabledAuthorizer),
            battery_backend: None,
            gpu_authorizer: Box::new(DisabledAuthorizer),
            gpu_backend: None,
            gpu_sender_fallback: None,
            product_gpu_authorizer: Box::new(DisabledAuthorizer),
            product_gpu_backend: None,
            fan_authorizer,
            fan_backend: Some(fan_backend),
            panel_authorizer: Box::new(DisabledAuthorizer),
            panel_backend: None,
            kb_authorizer: Box::new(DisabledAuthorizer),
            kb_backend: None,
            aura_authorizer: Box::new(DisabledAuthorizer),
            aura_backend: None,
        }
    }

    pub fn with_battery_gpu_and_fan_backends(
        authorizer: Box<dyn Authorizer>,
        battery_backend: Box<dyn BatteryMutationBackend>,
        battery_authorizer: Box<dyn Authorizer>,
        gpu_backend: Box<dyn supergfxd::SupergfxdMutationOperation>,
        gpu_authorizer: Box<dyn Authorizer>,
        fan_backend: Box<dyn fans::FanCurveMutationOperation>,
        fan_authorizer: Box<dyn Authorizer>,
    ) -> Self {
        Self {
            authorizer,
            writer: PlatformProfileWriter::default(),
            battery_authorizer,
            battery_backend: Some(battery_backend),
            gpu_authorizer,
            gpu_backend: Some(gpu_backend),
            gpu_sender_fallback: None,
            product_gpu_authorizer: Box::new(DisabledAuthorizer),
            product_gpu_backend: None,
            fan_authorizer,
            fan_backend: Some(fan_backend),
            panel_authorizer: Box::new(DisabledAuthorizer),
            panel_backend: None,
            kb_authorizer: Box::new(DisabledAuthorizer),
            kb_backend: None,
            aura_authorizer: Box::new(DisabledAuthorizer),
            aura_backend: None,
        }
    }

    pub fn with_gpu_backend(
        authorizer: Box<dyn Authorizer>,
        gpu_authorizer: Box<dyn Authorizer>,
        gpu_backend: Box<dyn supergfxd::SupergfxdMutationOperation>,
        p2p_sender: Option<String>,
    ) -> Self {
        Self {
            authorizer,
            writer: PlatformProfileWriter::default(),
            battery_authorizer: Box::new(DisabledAuthorizer),
            battery_backend: None,
            gpu_authorizer,
            gpu_backend: Some(gpu_backend),
            gpu_sender_fallback: p2p_sender,
            product_gpu_authorizer: Box::new(DisabledAuthorizer),
            product_gpu_backend: None,
            fan_authorizer: Box::new(DisabledAuthorizer),
            fan_backend: None,
            panel_authorizer: Box::new(DisabledAuthorizer),
            panel_backend: None,
            kb_authorizer: Box::new(DisabledAuthorizer),
            kb_backend: None,
            aura_authorizer: Box::new(DisabledAuthorizer),
            aura_backend: None,
        }
    }

    /// Attach the Panel Overdrive mutation backend to an existing service.
    ///
    /// Builder-style so production main can keep the proven combined
    /// constructor and attach the new capability without a 9-argument
    /// constructor.
    pub fn with_panel(
        mut self,
        panel_backend: Box<dyn panel::PanelOverdriveMutationBackend>,
        panel_authorizer: Box<dyn Authorizer>,
    ) -> Self {
        self.panel_backend = Some(panel_backend);
        self.panel_authorizer = panel_authorizer;
        self
    }

    /// Attach keyboard backlight brightness mutation backend.
    pub fn with_keyboard_backlight(
        mut self,
        kb_backend: Box<dyn keyboard_backlight::KeyboardBacklightMutationBackend>,
        kb_authorizer: Box<dyn Authorizer>,
    ) -> Self {
        self.kb_backend = Some(kb_backend);
        self.kb_authorizer = kb_authorizer;
        self
    }

    /// Attach the Aura Static RGB mutation backend to an existing service.
    ///
    /// Builder-style so production main can keep the proven combined
    /// constructor and attach the new capability without a 10-argument
    /// constructor.
    pub fn with_aura_static_rgb(
        mut self,
        aura_backend: Box<dyn aura::AuraStaticRgbMutationBackend>,
        aura_authorizer: Box<dyn Authorizer>,
    ) -> Self {
        self.aura_backend = Some(aura_backend);
        self.aura_authorizer = aura_authorizer;
        self
    }

    /// Attach the ASUS product-GPU queue backend explicitly. Production keeps
    /// this unattached until the capability-specific promotion gate is met.
    pub fn with_product_gpu_backend(
        mut self,
        backend: Box<dyn asus_gpu_mode::AsusProductGpuMutationOperation>,
        authorizer: Box<dyn Authorizer>,
    ) -> Self {
        self.product_gpu_backend = Some(backend);
        self.product_gpu_authorizer = authorizer;
        self
    }
}

struct DisabledAuthorizer;

#[async_trait]
impl Authorizer for DisabledAuthorizer {
    async fn authorize(&self, _sender: &str) -> Result<(), AuthorizeError> {
        Err(AuthorizeError::Failed(
            "battery backend not configured".into(),
        ))
    }
}

#[zbus::interface(name = "io.github.orbiscontrol.Hardware1")]
impl HardwareService {
    async fn set_performance_profile(
        &self,
        raw: u8,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<u8> {
        let sender = header
            .sender()
            .map(|s| s.to_string())
            .ok_or_else(|| zbus::fdo::Error::Failed("hardwared: sender отсутствует".into()))?;
        handle_set_performance_profile(self.authorizer.as_ref(), &self.writer, raw, &sender).await
    }

    /// Read-only typed evidence about Performance mutation backend availability.
    ///
    /// This is capability metadata, not a mutation: no authorization is
    /// required and no hardware write occurs. The returned wire value is one
    /// of `performance_mutation_wire::*`.
    fn performance_mutation_status(&self) -> u8 {
        use performance_mutation_wire;
        performance_mutation_wire::to_wire(self.writer.mutation_status())
    }

    /// Установить Battery configured threshold; возвращает подтверждённый
    /// configured percent, effective value остаётся отдельным read-model field.
    async fn set_charge_limit(
        &self,
        percent: u8,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<u8> {
        let sender = header
            .sender()
            .map(|s| s.to_string())
            .ok_or_else(|| zbus::fdo::Error::Failed("hardwared: sender отсутствует".into()))?;
        let backend = self
            .battery_backend
            .as_deref()
            .ok_or_else(|| zbus::fdo::Error::NotSupported("battery backend unavailable".into()))?;
        handle_set_charge_limit(self.battery_authorizer.as_ref(), backend, percent, &sender).await
    }

    async fn set_gpu_mode(
        &self,
        raw: u32,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<GpuMutationResult> {
        let sender = header
            .sender()
            .map(|s| s.to_string())
            .or_else(|| self.gpu_sender_fallback.clone())
            .ok_or_else(|| zbus::fdo::Error::Failed("hardwared: sender отсутствует".into()))?;
        handle_set_gpu_mode(
            self.gpu_authorizer.as_ref(),
            self.gpu_backend.as_deref(),
            raw,
            &sender,
        )
        .await
    }

    async fn set_product_gpu_mode(
        &self,
        raw: u32,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<ProductGpuMutationResult> {
        let sender = header
            .sender()
            .map(|s| s.to_string())
            .ok_or_else(|| zbus::fdo::Error::Failed("hardwared: sender отсутствует".into()))?;
        handle_set_product_gpu_mode(
            self.product_gpu_authorizer.as_ref(),
            self.product_gpu_backend.as_deref(),
            raw,
            &sender,
        )
        .await
    }

    /// Read-only typed evidence about Battery mutation backend availability.
    ///
    /// This is capability metadata, not a mutation: no authorization is
    /// required and no hardware I/O occurs. The returned wire value is one of
    /// `battery::battery_mutation_wire::*` so the GUI can honestly gate its
    /// mutation controls instead of guessing from `validate_charge_limit`.
    ///
    /// A startup `Supported` backend is re-checked for runtime asusd owner
    /// liveness on every query (#107): a stopped backend is reported as
    /// `TemporarilyUnavailable` and an inconclusive probe as `Unknown`.
    /// Non-`Supported` startup statuses are structural/authorization
    /// evidence and stay unchanged.
    async fn battery_mutation_status(&self) -> u8 {
        use battery::battery_mutation_wire;
        let startup = self
            .battery_backend
            .as_deref()
            .map(|b| b.mutation_status())
            .unwrap_or(battery::BatteryMutationStatus::Unknown);
        let status = match startup {
            battery::BatteryMutationStatus::Supported => {
                match self
                    .battery_backend
                    .as_deref()
                    .expect("backend present for Supported status")
                    .backend_alive()
                    .await
                {
                    Ok(true) => battery::BatteryMutationStatus::Supported,
                    Ok(false) => battery::BatteryMutationStatus::TemporarilyUnavailable,
                    Err(_) => battery::BatteryMutationStatus::Unknown,
                }
            }
            other => other,
        };
        battery_mutation_wire::to_wire(status)
    }

    /// Установить одну fan curve (profile wire 0..3, fan 0/1, ровно 8 точек);
    /// возвращает подтверждённый profile после asusd setter + read-back.
    async fn set_fan_curve(
        &self,
        profile_raw: u32,
        fan_raw: u8,
        curve_wire: fans::FanCurveWire,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<u32> {
        let sender = header
            .sender()
            .map(|s| s.to_string())
            .ok_or_else(|| zbus::fdo::Error::Failed("hardwared: sender отсутствует".into()))?;
        let backend = self
            .fan_backend
            .as_deref()
            .ok_or_else(|| zbus::fdo::Error::NotSupported("fan backend unavailable".into()))?;
        handle_set_fan_curve(
            self.fan_authorizer.as_ref(),
            backend,
            profile_raw,
            fan_raw,
            &curve_wire,
            &sender,
        )
        .await
    }

    /// Read-only typed evidence about fan curve mutation backend availability.
    ///
    /// This is capability metadata, not a mutation: no authorization is
    /// required and no fan write occurs. The probe uses the read-only
    /// `FanCurveData` path; when no fan backend is configured it reports
    /// `BackendMissing` (not configured ≠ proven Unsupported).
    async fn fan_mutation_status(&self) -> u8 {
        use fans::fan_mutation_wire;
        let status = match self.fan_backend.as_deref() {
            Some(backend) => backend.mutation_status().await,
            None => fans::FanMutationStatus::BackendMissing,
        };
        fan_mutation_wire::to_wire(status)
    }

    /// Restore factory fan curves for the whole ASUS profile. This uses the
    /// same typed fan mutation backend and polkit action as `SetFanCurve`.
    async fn reset_fan_curves_to_defaults(
        &self,
        profile_raw: u32,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<u32> {
        let sender = header
            .sender()
            .map(|s| s.to_string())
            .ok_or_else(|| zbus::fdo::Error::Failed("hardwared: sender отсутствует".into()))?;
        let backend = self
            .fan_backend
            .as_deref()
            .ok_or_else(|| zbus::fdo::Error::NotSupported("fan backend unavailable".into()))?;
        handle_reset_fan_curves_to_defaults(
            self.fan_authorizer.as_ref(),
            backend,
            profile_raw,
            &sender,
        )
        .await
    }

    /// Установить Panel Overdrive; возвращает подтверждённое observed значение
    /// (`0`/`1`) после typed asusd setter + authoritative fresh read-back.
    async fn set_panel_overdrive(
        &self,
        enabled: bool,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<u8> {
        let sender = header
            .sender()
            .map(|s| s.to_string())
            .ok_or_else(|| zbus::fdo::Error::Failed("hardwared: sender отсутствует".into()))?;
        let backend = self.panel_backend.as_deref().ok_or_else(|| {
            zbus::fdo::Error::NotSupported("panel overdrive backend unavailable".into())
        })?;
        panel::handle_set_panel_overdrive(self.panel_authorizer.as_ref(), backend, enabled, &sender)
            .await
    }

    /// Read-only typed evidence about Panel Overdrive mutation backend
    /// availability.
    ///
    /// This is capability metadata, not a mutation: no authorization is
    /// required and no hardware I/O occurs. The returned wire value is one of
    /// `panel::panel_mutation_wire::*`.
    fn panel_mutation_status(&self) -> u8 {
        use panel::panel_mutation_wire;
        panel_mutation_wire::to_wire(
            self.panel_backend
                .as_deref()
                .map(|b| b.mutation_status())
                .unwrap_or(panel::PanelOverdriveMutationStatus::Unknown),
        )
    }

    /// Установить keyboard backlight brightness.
    async fn set_keyboard_backlight(
        &self,
        level: u8,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<u8> {
        let sender = header
            .sender()
            .map(|s| s.to_string())
            .ok_or_else(|| zbus::fdo::Error::Failed("hardwared: sender отсутствует".into()))?;
        let backend = self.kb_backend.as_deref().ok_or_else(|| {
            zbus::fdo::Error::NotSupported("keyboard backlight backend unavailable".into())
        })?;
        keyboard_backlight::handle_set_keyboard_backlight(
            self.kb_authorizer.as_ref(),
            backend,
            level,
            &sender,
        )
        .await
    }

    /// Read-only typed evidence about keyboard backlight mutation backend.
    fn keyboard_backlight_mutation_status(&self) -> u8 {
        use keyboard_backlight::keyboard_backlight_mutation_wire;
        keyboard_backlight_mutation_wire::to_wire(
            self.kb_backend
                .as_deref()
                .map(|b| b.mutation_status())
                .unwrap_or(keyboard_backlight::KeyboardBacklightMutationStatus::Unknown),
        )
    }

    /// Установить Aura Static RGB; возвращает честный config-level результат
    /// (`Accepted`), hardware state не подтверждается (`kbd_rgb_mode`
    /// write-only).
    async fn set_aura_static_rgb(
        &self,
        r: u8,
        g: u8,
        b: u8,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<aura::AuraMutationResult> {
        let sender = header
            .sender()
            .map(|s| s.to_string())
            .ok_or_else(|| zbus::fdo::Error::Failed("hardwared: sender отсутствует".into()))?;
        let backend = self.aura_backend.as_deref().ok_or_else(|| {
            zbus::fdo::Error::NotSupported("aura static rgb backend unavailable".into())
        })?;
        aura::handle_set_aura_static_rgb(
            self.aura_authorizer.as_ref(),
            backend,
            AuraRgb { r, g, b },
            &sender,
        )
        .await
    }

    /// Read-only typed evidence about Aura Static RGB mutation backend
    /// availability.
    ///
    /// This is capability metadata, not a mutation: no authorization is
    /// required and no hardware I/O occurs. The returned wire value is one of
    /// `aura::aura_mutation_wire::*`.
    fn aura_mutation_status(&self) -> u8 {
        use aura::aura_mutation_wire;
        aura_mutation_wire::to_wire(
            self.aura_backend
                .as_deref()
                .map(|b| b.mutation_status())
                .unwrap_or(aura::AuraMutationStatus::Unknown),
        )
    }
}

#[zbus::proxy(
    interface = "io.github.orbiscontrol.Hardware1",
    default_service = "io.github.orbiscontrol.Hardware",
    default_path = "/io/github/orbiscontrol/Hardware"
)]
pub trait Hardware1 {
    fn set_performance_profile(&self, profile: u8) -> zbus::Result<u8>;
    /// Read-only typed Performance mutation backend availability (wire enum).
    fn performance_mutation_status(&self) -> zbus::Result<u8>;

    /// Установить Battery configured threshold; возвращает подтверждённый
    /// configured percent, effective value остаётся отдельным read-model field.
    fn set_charge_limit(&self, percent: u8) -> zbus::Result<u8>;
    fn set_gpu_mode(&self, requested_mode: u32) -> zbus::Result<GpuMutationResult>;
    fn set_product_gpu_mode(&self, requested_mode: u32) -> zbus::Result<ProductGpuMutationResult>;
    /// Read-only typed Battery mutation backend availability (wire enum).
    fn battery_mutation_status(&self) -> zbus::Result<u8>;

    /// Установить одну fan curve; возвращает подтверждённый profile wire.
    fn set_fan_curve(&self, profile: u32, fan: u8, curve: fans::FanCurveWire) -> zbus::Result<u32>;

    /// Read-only typed fan curve mutation backend availability (wire enum).
    fn fan_mutation_status(&self) -> zbus::Result<u8>;

    /// Restore platform factory fan curves for the whole lossless ASUS profile.
    fn reset_fan_curves_to_defaults(&self, profile: u32) -> zbus::Result<u32>;

    /// Установить Panel Overdrive; возвращает подтверждённое observed значение
    /// (`0`/`1`) после typed asusd setter + authoritative fresh read-back.
    fn set_panel_overdrive(&self, enabled: bool) -> zbus::Result<u8>;

    /// Read-only typed Panel Overdrive mutation backend availability (wire enum).
    fn panel_mutation_status(&self) -> zbus::Result<u8>;

    /// Установить keyboard backlight brightness.
    fn set_keyboard_backlight(&self, level: u8) -> zbus::Result<u8>;

    /// Read-only typed keyboard backlight mutation backend availability.
    fn keyboard_backlight_mutation_status(&self) -> zbus::Result<u8>;

    /// Установить Aura Static RGB; возвращает честный config-level результат
    /// (`Accepted`), hardware state не подтверждается (`kbd_rgb_mode`
    /// write-only).
    fn set_aura_static_rgb(&self, r: u8, g: u8, b: u8) -> zbus::Result<aura::AuraMutationResult>;

    /// Read-only typed Aura Static RGB mutation backend availability (wire enum).
    fn aura_mutation_status(&self) -> zbus::Result<u8>;
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct ScriptedIo {
        choices: Mutex<String>,
        profile: Mutex<String>,
        reads: AtomicUsize,
        writes: AtomicUsize,
        write_error: Mutex<Option<std::io::Error>>,
        read_error: Mutex<Option<std::io::Error>>,
        override_read_back: Mutex<Option<String>>,
    }

    impl ScriptedIo {
        fn new(choices: &str, profile: &str) -> Self {
            Self {
                choices: Mutex::new(choices.to_string()),
                profile: Mutex::new(profile.to_string()),
                reads: AtomicUsize::new(0),
                writes: AtomicUsize::new(0),
                write_error: Mutex::new(None),
                read_error: Mutex::new(None),
                override_read_back: Mutex::new(None),
            }
        }

        fn reads(&self) -> usize {
            self.reads.load(Ordering::SeqCst)
        }

        fn writes(&self) -> usize {
            self.writes.load(Ordering::SeqCst)
        }

        fn profile(&self) -> String {
            self.profile.lock().unwrap().clone()
        }

        fn set_write_error(&self, err: std::io::Error) {
            *self.write_error.lock().unwrap() = Some(err);
        }

        fn set_read_error(&self, err: std::io::Error) {
            *self.read_error.lock().unwrap() = Some(err);
        }

        fn set_override_read_back(&self, value: &str) {
            *self.override_read_back.lock().unwrap() = Some(value.to_string());
        }

        fn clear_override_read_back(&self) {
            *self.override_read_back.lock().unwrap() = None;
        }

        fn set_choices(&self, choices: &str) {
            *self.choices.lock().unwrap() = choices.to_string();
        }
    }

    impl ProfileIo for ScriptedIo {
        fn read_to_string(&self, path: &Path) -> Result<String, ProviderError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            if let Some(err) = self.read_error.lock().unwrap().as_ref() {
                // Клонировать io::Error нельзя; воспроизводим с тем же kind.
                let kind = err.kind();
                return Err(ProviderError::Io(std::io::Error::new(
                    kind,
                    err.to_string(),
                )));
            }
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if name.ends_with("platform_profile_choices") {
                return Ok(self.choices.lock().unwrap().clone());
            }
            if let Some(over) = self.override_read_back.lock().unwrap().as_ref() {
                return Ok(over.clone());
            }
            Ok(self.profile.lock().unwrap().clone())
        }

        fn write(&self, _path: &Path, content: &str) -> Result<(), ProviderError> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            if let Some(err) = self.write_error.lock().unwrap().as_ref() {
                return Err(ProviderError::Io(std::io::Error::other(err.to_string())));
            }
            *self.profile.lock().unwrap() = content.to_string();
            Ok(())
        }
    }

    fn writer(io: ScriptedIo) -> PlatformProfileWriter<ScriptedIo> {
        PlatformProfileWriter::with_io(
            io,
            PathBuf::from("/tmp/test-platform_profile"),
            PathBuf::from("/tmp/test-platform_profile_choices"),
        )
    }

    #[test]
    fn production_paths_are_fixed() {
        assert_eq!(PLATFORM_PROFILE_PATH, "/sys/firmware/acpi/platform_profile");
        assert_eq!(
            PLATFORM_PROFILE_CHOICES_PATH,
            "/sys/firmware/acpi/platform_profile_choices"
        );
    }

    #[test]
    fn performance_mutation_status_classifies_read_evidence() {
        // Present, non-empty ABI → Supported.
        let io = ScriptedIo::new("quiet balanced performance", "balanced");
        let w = writer(io);
        assert_eq!(w.mutation_status(), PerformanceMutationStatus::Supported);

        // Empty choices file is malformed → Unknown, never guessed Supported.
        let io = ScriptedIo::new("", "balanced");
        let w = writer(io);
        assert_eq!(w.mutation_status(), PerformanceMutationStatus::Unknown);

        // Structurally absent ABI → Unsupported.
        let io = ScriptedIo::new("quiet balanced performance", "balanced");
        io.set_read_error(std::io::Error::new(std::io::ErrorKind::NotFound, "missing"));
        let w = writer(io);
        assert_eq!(w.mutation_status(), PerformanceMutationStatus::Unsupported);

        // Permission denied on the ABI read → PermissionDenied.
        let io = ScriptedIo::new("quiet balanced performance", "balanced");
        io.set_read_error(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "denied",
        ));
        let w = writer(io);
        assert_eq!(
            w.mutation_status(),
            PerformanceMutationStatus::PermissionDenied
        );

        // Transient read failure → TemporarilyUnavailable.
        let io = ScriptedIo::new("quiet balanced performance", "balanced");
        io.set_read_error(std::io::Error::other("temporary"));
        let w = writer(io);
        assert_eq!(
            w.mutation_status(),
            PerformanceMutationStatus::TemporarilyUnavailable
        );
    }

    #[test]
    fn performance_mutation_wire_roundtrip_is_total() {
        for status in [
            PerformanceMutationStatus::Supported,
            PerformanceMutationStatus::Unsupported,
            PerformanceMutationStatus::TemporarilyUnavailable,
            PerformanceMutationStatus::PermissionDenied,
            PerformanceMutationStatus::Unknown,
        ] {
            let wire = performance_mutation_wire::to_wire(status);
            assert_eq!(performance_mutation_wire::from_wire(wire), Some(status));
        }
        assert_eq!(performance_mutation_wire::from_wire(99), None);
    }

    #[test]
    fn exact_symbol_mapping_is_total() {
        assert_eq!(profile_symbol(PerformanceProfile::Silent), "quiet");
        assert_eq!(profile_symbol(PerformanceProfile::Balanced), "balanced");
        assert_eq!(profile_symbol(PerformanceProfile::Turbo), "performance");
        for p in PerformanceProfile::ALL {
            let s = profile_symbol(p);
            assert!(["quiet", "balanced", "performance"].contains(&s));
        }
    }

    #[test]
    fn present_choice_writes_once_and_reads_back() {
        let io = ScriptedIo::new("quiet balanced performance", "balanced");
        let w = writer(io);
        let res = w
            .set_performance_profile(PerformanceProfile::Silent)
            .expect("silent доступен");
        assert_eq!(res, ApplyResult::Applied);
        assert_eq!(w.io.writes(), 1);
        assert_eq!(w.io.profile(), "quiet\n");
        assert_eq!(w.io.reads(), 2);
    }

    #[test]
    fn missing_choice_is_unsupported_with_zero_writes() {
        let io = ScriptedIo::new("balanced performance", "balanced");
        let w = writer(io);
        let err = w
            .set_performance_profile(PerformanceProfile::Silent)
            .expect_err("quiet отсутствует в choices");
        assert!(matches!(err, ProviderError::Unsupported(_)));
        assert_eq!(w.io.writes(), 0);
        assert_eq!(w.io.profile(), "balanced");
    }

    #[test]
    fn write_error_propagates() {
        let io = ScriptedIo::new("quiet balanced performance", "balanced");
        io.set_write_error(std::io::Error::other("simulated io failure"));
        let w = writer(io);
        let err = w
            .set_performance_profile(PerformanceProfile::Balanced)
            .expect_err("write error");
        assert!(matches!(err, ProviderError::Io(_)));
        assert_eq!(w.io.writes(), 1);
    }

    #[test]
    fn read_back_mismatch_is_not_applied() {
        let io = ScriptedIo::new("quiet balanced performance", "balanced");
        io.set_override_read_back("performance");
        let w = writer(io);
        let err = w
            .set_performance_profile(PerformanceProfile::Silent)
            .expect_err("read-back mismatch");
        assert!(matches!(err, ProviderError::Conflict(_)));
        assert_eq!(w.io.writes(), 1);
    }

    #[test]
    fn empty_choices_are_internal() {
        let io = ScriptedIo::new("", "balanced");
        let w = writer(io);
        let err = w
            .set_performance_profile(PerformanceProfile::Balanced)
            .expect_err("empty choices");
        assert!(matches!(err, ProviderError::Internal(_)));
        assert_eq!(w.io.writes(), 0);
    }

    #[test]
    fn reads_are_fresh_not_cached() {
        let io = ScriptedIo::new("quiet balanced performance", "balanced");
        let w = writer(io);
        assert_eq!(
            w.set_performance_profile(PerformanceProfile::Silent)
                .expect("first"),
            ApplyResult::Applied
        );
        w.io.set_choices("balanced performance");
        w.io.clear_override_read_back();
        *w.io.profile.lock().unwrap() = "balanced".to_string();
        let err = w
            .set_performance_profile(PerformanceProfile::Silent)
            .expect_err("fresh choices без quiet");
        assert!(matches!(err, ProviderError::Unsupported(_)));
        assert_eq!(w.io.writes(), 1);
    }

    #[derive(Clone, Copy)]
    enum AuthOutcome {
        Ok,
        Denied,
        Failed,
    }

    struct FakeAuthorizer {
        outcome: Mutex<AuthOutcome>,
        calls: AtomicUsize,
        last_sender: Mutex<Option<String>>,
    }

    impl FakeAuthorizer {
        fn new(outcome: AuthOutcome) -> Self {
            Self {
                outcome: Mutex::new(outcome),
                calls: AtomicUsize::new(0),
                last_sender: Mutex::new(None),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }

        fn last_sender(&self) -> Option<String> {
            self.last_sender.lock().unwrap().clone()
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

    #[test]
    fn wire_decode_strict() {
        assert_eq!(
            profile_from_wire(wire::SILENT).unwrap(),
            PerformanceProfile::Silent
        );
        assert_eq!(
            profile_from_wire(wire::BALANCED).unwrap(),
            PerformanceProfile::Balanced
        );
        assert_eq!(
            profile_from_wire(wire::TURBO).unwrap(),
            PerformanceProfile::Turbo
        );
        assert!(matches!(
            profile_from_wire(5),
            Err(ProviderError::InvalidRequest(_))
        ));
    }

    #[test]
    fn wire_roundtrip() {
        for p in PerformanceProfile::ALL {
            assert_eq!(profile_from_wire(profile_to_wire(p)).unwrap(), p);
        }
    }

    #[tokio::test]
    async fn unknown_wire_is_invalid_args_with_zero_calls() {
        let io = ScriptedIo::new("quiet balanced performance", "balanced");
        let w = writer(io);
        let auth = FakeAuthorizer::new(AuthOutcome::Ok);
        let err = handle_set_performance_profile(&auth, &w, 99, ":1.1")
            .await
            .expect_err("unknown wire");
        assert!(matches!(err, zbus::fdo::Error::InvalidArgs(_)));
        assert_eq!(w.io.writes(), 0);
        assert_eq!(auth.calls(), 0);
    }

    struct FakeProductGpuBackend {
        calls: AtomicUsize,
        result: Mutex<Option<Result<asus_gpu_mode::AsusGpuMutationReadback, ProviderError>>>,
    }

    #[async_trait]
    impl asus_gpu_mode::AsusProductGpuMutationOperation for FakeProductGpuBackend {
        async fn set_mode(
            &self,
            _requested: AsusGpuMode,
        ) -> Result<asus_gpu_mode::AsusGpuMutationReadback, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.result.lock().unwrap().take().unwrap()
        }
    }

    fn product_gpu_readback() -> asus_gpu_mode::AsusGpuMutationReadback {
        let snapshot = orbis_providers::asus_gpu_mode::AsusGpuModeSnapshot::from_values(
            Some(0),
            Some(1),
            Some(1),
            Some(1),
        );
        asus_gpu_mode::AsusGpuMutationReadback {
            requested: AsusGpuMode::Integrated,
            outcome: ProductGpuOutcome::RebootRequired,
            snapshot,
        }
    }

    #[tokio::test]
    async fn product_gpu_unknown_wire_is_invalid_args_before_authorization() {
        assert_eq!(
            PRODUCT_GPU_POLKIT_ACTION,
            "io.github.orbiscontrol.hardware.set-product-gpu-mode"
        );
        let auth = FakeAuthorizer::new(AuthOutcome::Ok);
        let backend = FakeProductGpuBackend {
            calls: AtomicUsize::new(0),
            result: Mutex::new(Some(Ok(product_gpu_readback()))),
        };

        let error = handle_set_product_gpu_mode(&auth, Some(&backend), 99, ":1.42")
            .await
            .expect_err("unknown product GPU wire value");

        assert!(matches!(error, zbus::fdo::Error::InvalidArgs(_)));
        assert_eq!(auth.calls(), 0);
        assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn product_gpu_authorization_happens_before_backend_call() {
        let auth = FakeAuthorizer::new(AuthOutcome::Denied);
        let backend = FakeProductGpuBackend {
            calls: AtomicUsize::new(0),
            result: Mutex::new(Some(Ok(product_gpu_readback()))),
        };

        let error = handle_set_product_gpu_mode(&auth, Some(&backend), 1, ":1.42")
            .await
            .expect_err("denied product GPU request");

        assert!(matches!(error, zbus::fdo::Error::AccessDenied(_)));
        assert_eq!(auth.last_sender(), Some(":1.42".to_string()));
        assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn product_gpu_result_uses_typed_wire_fields() {
        let auth = FakeAuthorizer::new(AuthOutcome::Ok);
        let backend = FakeProductGpuBackend {
            calls: AtomicUsize::new(0),
            result: Mutex::new(Some(Ok(product_gpu_readback()))),
        };

        let result = handle_set_product_gpu_mode(&auth, Some(&backend), 1, ":1.42")
            .await
            .expect("product GPU queue result");

        assert_eq!(result.requested_mode, 1);
        assert_eq!(result.current_mode, 0);
        assert_eq!(result.queued_mode, 1);
        assert_eq!(result.outcome, 1);
        assert!(result.reboot_required);
        assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn authorization_denied_zero_writer_calls() {
        let io = ScriptedIo::new("quiet balanced performance", "balanced");
        let w = writer(io);
        let auth = FakeAuthorizer::new(AuthOutcome::Denied);
        let err = handle_set_performance_profile(&auth, &w, wire::SILENT, ":1.42")
            .await
            .expect_err("denied");
        assert!(matches!(err, zbus::fdo::Error::AccessDenied(_)));
        assert_eq!(w.io.writes(), 0);
        assert_eq!(auth.last_sender(), Some(":1.42".to_string()));
    }

    #[tokio::test]
    async fn authorization_failed_zero_writer_calls() {
        let io = ScriptedIo::new("quiet balanced performance", "balanced");
        let w = writer(io);
        let auth = FakeAuthorizer::new(AuthOutcome::Failed);
        let err = handle_set_performance_profile(&auth, &w, wire::BALANCED, ":1.7")
            .await
            .expect_err("polkit down");
        assert!(matches!(err, zbus::fdo::Error::Failed(_)));
        assert_eq!(w.io.writes(), 0);
    }

    #[tokio::test]
    async fn authorized_calls_writer_once_and_returns_confirmed() {
        let io = ScriptedIo::new("quiet balanced performance", "balanced");
        let w = writer(io);
        let auth = FakeAuthorizer::new(AuthOutcome::Ok);
        let confirmed = handle_set_performance_profile(&auth, &w, wire::SILENT, ":1.42")
            .await
            .expect("authorized");
        assert_eq!(confirmed, wire::SILENT);
        assert_eq!(w.io.writes(), 1);
        assert_eq!(w.io.profile(), "quiet\n");
    }

    #[tokio::test]
    async fn writer_failure_is_failed_not_unsupported() {
        let io = ScriptedIo::new("quiet balanced performance", "balanced");
        io.set_write_error(std::io::Error::other("simulated write failure"));
        let w = writer(io);
        let auth = FakeAuthorizer::new(AuthOutcome::Ok);
        let err = handle_set_performance_profile(&auth, &w, wire::BALANCED, ":1.42")
            .await
            .expect_err("write failure");
        assert!(matches!(err, zbus::fdo::Error::Failed(_)));
        assert!(!matches!(err, zbus::fdo::Error::NotSupported(_)));
    }

    struct FakeBatteryBackend {
        calls: AtomicUsize,
        outcome: Mutex<Result<BatteryMutationReadback, ProviderError>>,
        alive: Result<bool, ()>,
    }

    #[async_trait]
    impl BatteryMutationBackend for FakeBatteryBackend {
        async fn set_charge_limit(
            &self,
            _percent: u8,
        ) -> Result<BatteryMutationReadback, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match &*self.outcome.lock().unwrap() {
                Ok(readback) => Ok(readback.clone()),
                Err(error) => Err(ProviderError::Internal(error.to_string())),
            }
        }

        fn mutation_status(&self) -> battery::BatteryMutationStatus {
            battery::BatteryMutationStatus::Supported
        }

        async fn backend_alive(&self) -> Result<bool, ProviderError> {
            self.alive
                .map_err(|()| ProviderError::Dbus("liveness probe failed".into()))
        }
    }

    fn battery_backend(configured: u8, effective: u8) -> FakeBatteryBackend {
        FakeBatteryBackend {
            calls: AtomicUsize::new(0),
            alive: Ok(true),
            outcome: Mutex::new(Ok(BatteryMutationReadback {
                requested_percent: configured,
                configured_percent: configured,
                effective_percent: effective,
                result: ApplyResult::Applied,
            })),
        }
    }

    #[tokio::test]
    async fn battery_invalid_range_is_rejected_before_authorization_or_backend() {
        for percent in [19, 101] {
            let backend = battery_backend(80, 100);
            let authorizer = FakeAuthorizer::new(AuthOutcome::Ok);
            let error = handle_set_charge_limit(&authorizer, &backend, percent, ":1.90")
                .await
                .expect_err("invalid battery range");
            assert!(matches!(error, zbus::fdo::Error::InvalidArgs(_)));
            assert_eq!(authorizer.calls(), 0);
            assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
        }
    }

    #[tokio::test]
    async fn battery_denied_challenge_and_auth_failure_do_not_mutate() {
        for outcome in [AuthOutcome::Denied, AuthOutcome::Failed] {
            let backend = battery_backend(80, 100);
            let authorizer = FakeAuthorizer::new(outcome);
            let error = handle_set_charge_limit(&authorizer, &backend, 80, ":1.91")
                .await
                .expect_err("authorization failure");
            assert!(matches!(
                (outcome, error),
                (AuthOutcome::Denied, zbus::fdo::Error::AccessDenied(_))
                    | (AuthOutcome::Failed, zbus::fdo::Error::Failed(_))
            ));
            assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
        }
    }

    #[tokio::test]
    async fn battery_authorized_returns_configured_for_20_and_100() {
        for percent in [20, 100] {
            let backend = battery_backend(percent, 100);
            let authorizer = FakeAuthorizer::new(AuthOutcome::Ok);
            let confirmed = handle_set_charge_limit(&authorizer, &backend, percent, ":1.92")
                .await
                .expect("authorized battery mutation");
            assert_eq!(confirmed, percent);
            assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
            assert_eq!(authorizer.last_sender(), Some(":1.92".into()));
        }
    }

    #[tokio::test]
    async fn battery_backend_error_maps_to_failed_without_retry() {
        let backend = FakeBatteryBackend {
            calls: AtomicUsize::new(0),
            alive: Ok(true),
            outcome: Mutex::new(Err(ProviderError::Dbus("asusd failed".into()))),
        };
        let authorizer = FakeAuthorizer::new(AuthOutcome::Ok);
        let error = handle_set_charge_limit(&authorizer, &backend, 80, ":1.93")
            .await
            .expect_err("backend failure");
        assert!(matches!(error, zbus::fdo::Error::Failed(_)));
        assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn battery_effective_divergence_is_not_boundary_failure() {
        let backend = battery_backend(80, 100);
        let authorizer = FakeAuthorizer::new(AuthOutcome::Ok);
        assert_eq!(
            handle_set_charge_limit(&authorizer, &backend, 80, ":1.94")
                .await
                .expect("configured read-back confirmed"),
            80
        );
    }

    #[tokio::test]
    async fn battery_mutation_status_reflects_backend_presence() {
        // Proven backend installed → SUPPORTED wire evidence.
        let service = HardwareService::with_battery_backend(
            Box::new(FakeAuthorizer::new(AuthOutcome::Ok)),
            Box::new(battery_backend(80, 100)),
            Box::new(FakeAuthorizer::new(AuthOutcome::Ok)),
        );
        assert_eq!(
            service.battery_mutation_status().await,
            battery::battery_mutation_wire::SUPPORTED
        );

        // No battery backend configured → UNKNOWN wire (no evidence, never a
        // guessed Supported).
        let service = HardwareService::new(Box::new(FakeAuthorizer::new(AuthOutcome::Ok)));
        assert_eq!(
            service.battery_mutation_status().await,
            battery::battery_mutation_wire::UNKNOWN
        );
    }

    #[tokio::test]
    async fn battery_mutation_status_demotes_stale_supported_on_owner_loss() {
        // #107: a startup-Supported backend whose asusd owner disappeared is
        // reported dynamically as TemporarilyUnavailable, never Supported.
        let mut backend = battery_backend(80, 100);
        backend.alive = Ok(false);
        let service = HardwareService::with_battery_backend(
            Box::new(FakeAuthorizer::new(AuthOutcome::Ok)),
            Box::new(backend),
            Box::new(FakeAuthorizer::new(AuthOutcome::Ok)),
        );
        assert_eq!(
            service.battery_mutation_status().await,
            battery::battery_mutation_wire::TEMPORARILY_UNAVAILABLE
        );
    }

    #[tokio::test]
    async fn battery_mutation_status_inconclusive_liveness_probe_is_unknown() {
        // A failed liveness probe is no evidence: Unknown, not Supported.
        let mut backend = battery_backend(80, 100);
        backend.alive = Err(());
        let service = HardwareService::with_battery_backend(
            Box::new(FakeAuthorizer::new(AuthOutcome::Ok)),
            Box::new(backend),
            Box::new(FakeAuthorizer::new(AuthOutcome::Ok)),
        );
        assert_eq!(
            service.battery_mutation_status().await,
            battery::battery_mutation_wire::UNKNOWN
        );
    }

    #[tokio::test]
    async fn writer_error_maps_to_dbus_error() {
        let io = ScriptedIo::new("quiet balanced performance", "balanced");
        io.set_write_error(std::io::Error::other("simulated io failure"));
        let w = writer(io);
        let auth = FakeAuthorizer::new(AuthOutcome::Ok);
        let err = handle_set_performance_profile(&auth, &w, wire::BALANCED, ":1.42")
            .await
            .expect_err("writer io error");
        assert!(matches!(err, zbus::fdo::Error::Failed(_)));
    }

    #[tokio::test]
    async fn writer_read_back_mismatch_is_error_not_success() {
        let io = ScriptedIo::new("quiet balanced performance", "balanced");
        io.set_override_read_back("performance");
        let w = writer(io);
        let auth = FakeAuthorizer::new(AuthOutcome::Ok);
        let err = handle_set_performance_profile(&auth, &w, wire::SILENT, ":1.42")
            .await
            .expect_err("read-back mismatch");
        assert!(matches!(err, zbus::fdo::Error::Failed(_)));
    }

    #[test]
    fn dbus_names_are_stable() {
        assert_eq!(DBUS_NAME, "io.github.orbiscontrol.Hardware");
        assert_eq!(DBUS_OBJECT_PATH, "/io/github/orbiscontrol/Hardware");
        assert_eq!(DBUS_INTERFACE_NAME, "io.github.orbiscontrol.Hardware1");
        assert_eq!(
            POLKIT_ACTION,
            "io.github.orbiscontrol.hardware.set-performance-profile"
        );
        assert_eq!(
            BATTERY_POLKIT_ACTION,
            "io.github.orbiscontrol.hardware.set-charge-limit"
        );
        assert_eq!(
            GPU_POLKIT_ACTION,
            "io.github.orbiscontrol.hardware.set-gpu-mode"
        );
        assert_eq!(
            FAN_POLKIT_ACTION,
            "io.github.orbiscontrol.hardware.set-fan-curve"
        );
    }

    struct FakeFanBackend {
        set_calls: AtomicUsize,
        reset_calls: AtomicUsize,
        fail: std::sync::atomic::AtomicBool,
        status: fans::FanMutationStatus,
    }

    impl FakeFanBackend {
        fn new() -> Self {
            Self {
                set_calls: AtomicUsize::new(0),
                reset_calls: AtomicUsize::new(0),
                fail: std::sync::atomic::AtomicBool::new(false),
                status: fans::FanMutationStatus::Supported,
            }
        }

        fn with_status(status: fans::FanMutationStatus) -> Self {
            Self {
                set_calls: AtomicUsize::new(0),
                reset_calls: AtomicUsize::new(0),
                fail: std::sync::atomic::AtomicBool::new(false),
                status,
            }
        }
    }

    #[async_trait]
    impl fans::FanCurveMutationOperation for FakeFanBackend {
        async fn set_fan_curve(
            &self,
            profile: orbis_core::profile::AsusdFanProfile,
            fan: &orbis_core::fan::FanId,
            _curve: &fans::FanCurvePoints,
        ) -> Result<fans::FanCurveMutationReadback, ProviderError> {
            self.set_calls.fetch_add(1, Ordering::SeqCst);
            if self.fail.load(Ordering::SeqCst) {
                return Err(ProviderError::BackendUnavailable("backend down".into()));
            }
            Ok(fans::FanCurveMutationReadback {
                requested_profile: profile,
                requested_fan: fan.clone(),
                result: ApplyResult::Applied,
            })
        }

        async fn mutation_status(&self) -> fans::FanMutationStatus {
            self.status
        }

        async fn reset_curves_to_defaults(
            &self,
            profile: orbis_core::profile::AsusdFanProfile,
        ) -> Result<fans::FanCurveDefaultsReadback, ProviderError> {
            self.reset_calls.fetch_add(1, Ordering::SeqCst);
            if self.fail.load(Ordering::SeqCst) {
                return Err(ProviderError::BackendUnavailable("backend down".into()));
            }
            Ok(fans::FanCurveDefaultsReadback {
                requested_profile: profile,
                result: ApplyResult::Applied,
                observed_curves: 2,
            })
        }
    }

    fn fan_curve_wire() -> fans::FanCurveWire {
        fans::FanCurveWire {
            temps: vec![45, 49, 54, 68, 74, 79, 84, 89],
            pwms: vec![5, 22, 38, 45, 56, 63, 81, 94],
        }
    }

    #[tokio::test]
    async fn fan_curve_unknown_profile_is_invalid_args_with_zero_calls() {
        let backend = FakeFanBackend::new();
        let auth = FakeAuthorizer::new(AuthOutcome::Ok);
        let err = handle_set_fan_curve(&auth, &backend, 99, 0, &fan_curve_wire(), ":1.1")
            .await
            .expect_err("unknown profile");
        assert!(matches!(err, zbus::fdo::Error::InvalidArgs(_)));
        assert_eq!(backend.set_calls.load(Ordering::SeqCst), 0);
        assert_eq!(auth.calls(), 0);
    }

    #[tokio::test]
    async fn fan_curve_unknown_fan_is_invalid_args_with_zero_calls() {
        let backend = FakeFanBackend::new();
        let auth = FakeAuthorizer::new(AuthOutcome::Ok);
        let err = handle_set_fan_curve(&auth, &backend, 0, 2, &fan_curve_wire(), ":1.1")
            .await
            .expect_err("unknown fan");
        assert!(matches!(err, zbus::fdo::Error::InvalidArgs(_)));
        assert_eq!(backend.set_calls.load(Ordering::SeqCst), 0);
        assert_eq!(auth.calls(), 0);
    }

    #[tokio::test]
    async fn fan_curve_invalid_curve_is_invalid_args_with_zero_calls() {
        let backend = FakeFanBackend::new();
        let auth = FakeAuthorizer::new(AuthOutcome::Ok);
        let bad = fans::FanCurveWire {
            temps: vec![45, 49, 54, 68, 74, 79, 84, 89],
            pwms: vec![50, 22, 38, 45, 56, 63, 81, 94],
        };
        let err = handle_set_fan_curve(&auth, &backend, 0, 0, &bad, ":1.1")
            .await
            .expect_err("invalid curve");
        assert!(matches!(err, zbus::fdo::Error::InvalidArgs(_)));
        assert_eq!(backend.set_calls.load(Ordering::SeqCst), 0);
        assert_eq!(auth.calls(), 0);
    }

    #[tokio::test]
    async fn fan_curve_unauthorized_does_not_reach_backend() {
        let backend = FakeFanBackend::new();
        let auth = FakeAuthorizer::new(AuthOutcome::Denied);
        let err = handle_set_fan_curve(&auth, &backend, 0, 0, &fan_curve_wire(), ":1.1")
            .await
            .expect_err("denied");
        assert!(matches!(err, zbus::fdo::Error::AccessDenied(_)));
        assert_eq!(backend.set_calls.load(Ordering::SeqCst), 0);
        assert_eq!(auth.calls(), 1);
    }

    #[tokio::test]
    async fn fan_curve_success_returns_confirmed_profile() {
        let backend = FakeFanBackend::new();
        let auth = FakeAuthorizer::new(AuthOutcome::Ok);
        let confirmed = handle_set_fan_curve(&auth, &backend, 2, 0, &fan_curve_wire(), ":1.1")
            .await
            .expect("success");
        assert_eq!(confirmed, 2);
        assert_eq!(backend.set_calls.load(Ordering::SeqCst), 1);
        assert_eq!(auth.calls(), 1);
        assert_eq!(auth.last_sender(), Some(":1.1".to_string()));
    }

    #[tokio::test]
    async fn fan_curve_backend_error_propagates() {
        let backend = FakeFanBackend::new();
        backend.fail.store(true, Ordering::SeqCst);
        let auth = FakeAuthorizer::new(AuthOutcome::Ok);
        let err = handle_set_fan_curve(&auth, &backend, 0, 0, &fan_curve_wire(), ":1.1")
            .await
            .expect_err("backend error");
        assert!(matches!(err, zbus::fdo::Error::Failed(_)));
        assert_eq!(backend.set_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn fan_mutation_status_reflects_backend_evidence() {
        use fans::fan_mutation_wire;
        // No fan backend configured → BackendMissing wire (not configured ≠
        // proven Unsupported).
        let service = HardwareService::new(Box::new(FakeAuthorizer::new(AuthOutcome::Ok)));
        assert_eq!(
            service.fan_mutation_status().await,
            fan_mutation_wire::BACKEND_MISSING
        );

        // Configured backend reporting Supported → SUPPORTED wire.
        let service = HardwareService::with_fan_backend(
            Box::new(FakeAuthorizer::new(AuthOutcome::Ok)),
            Box::new(FakeFanBackend::with_status(
                fans::FanMutationStatus::Supported,
            )),
            Box::new(FakeAuthorizer::new(AuthOutcome::Ok)),
        );
        assert_eq!(
            service.fan_mutation_status().await,
            fan_mutation_wire::SUPPORTED
        );

        // Configured backend reporting PermissionDenied → PERMISSION_DENIED wire.
        let service = HardwareService::with_fan_backend(
            Box::new(FakeAuthorizer::new(AuthOutcome::Ok)),
            Box::new(FakeFanBackend::with_status(
                fans::FanMutationStatus::PermissionDenied,
            )),
            Box::new(FakeAuthorizer::new(AuthOutcome::Ok)),
        );
        assert_eq!(
            service.fan_mutation_status().await,
            fan_mutation_wire::PERMISSION_DENIED
        );
    }

    #[tokio::test]
    async fn fan_defaults_unknown_profile_is_rejected_before_auth() {
        let backend = FakeFanBackend::new();
        let auth = FakeAuthorizer::new(AuthOutcome::Ok);
        let err = handle_reset_fan_curves_to_defaults(&auth, &backend, 99, ":1.1")
            .await
            .expect_err("unknown profile");
        assert!(matches!(err, zbus::fdo::Error::InvalidArgs(_)));
        assert_eq!(auth.calls(), 0);
        assert_eq!(backend.reset_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn fan_defaults_denied_does_not_reach_backend() {
        let backend = FakeFanBackend::new();
        let auth = FakeAuthorizer::new(AuthOutcome::Denied);
        let err = handle_reset_fan_curves_to_defaults(&auth, &backend, 3, ":1.8")
            .await
            .expect_err("denied");
        assert!(matches!(err, zbus::fdo::Error::AccessDenied(_)));
        assert_eq!(backend.reset_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn fan_defaults_success_returns_lossless_profile() {
        let backend = FakeFanBackend::new();
        let auth = FakeAuthorizer::new(AuthOutcome::Ok);
        let confirmed = handle_reset_fan_curves_to_defaults(&auth, &backend, 3, ":1.8")
            .await
            .expect("reset");
        assert_eq!(confirmed, 3);
        assert_eq!(backend.reset_calls.load(Ordering::SeqCst), 1);
        assert_eq!(auth.last_sender(), Some(":1.8".into()));
    }

    #[test]
    fn polkit_policy_contains_all_mutation_actions() {
        let policy_path = option_env!("ORBIS_HARDWARED_POLKIT_POLICY").unwrap_or(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../packaging/nix/polkit/io.github.orbiscontrol.hardware.policy"
        ));
        let policy = std::fs::read_to_string(policy_path)
            .unwrap_or_else(|e| panic!("не удалось прочитать policy файл {policy_path}: {e}"));

        for action in [
            POLKIT_ACTION,
            BATTERY_POLKIT_ACTION,
            GPU_POLKIT_ACTION,
            FAN_POLKIT_ACTION,
        ] {
            assert!(
                policy.contains(&format!("<action id=\"{action}\">")),
                "policy файл не содержит action '{action}'"
            );
        }
    }
}
