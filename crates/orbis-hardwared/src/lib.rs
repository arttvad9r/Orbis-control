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
//! - D-Bus/polkit/activation/identity contract — следующий шаг (ADR 0006);
//!   этот crate не выполняет I/O к реальному sysfs в тестах (fake backend).

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use orbis_core::action::ApplyResult;
use orbis_core::profile::PerformanceProfile;
use orbis_providers::error::ProviderError;

#[allow(dead_code)]
mod battery;

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

        // 1. fresh choices read.
        let choices_raw = self.read_trimmed(&self.choices_path, "platform_profile_choices")?;
        let choices: Vec<&str> = choices_raw.split_whitespace().collect();
        if choices.is_empty() {
            return Err(ProviderError::Internal(
                "hardwared: platform_profile_choices не содержит ни одного символа".into(),
            ));
        }

        // 2. requested symbol должен присутствовать.
        if !choices.contains(&symbol) {
            return Err(ProviderError::Unsupported(format!(
                "hardwared: profile symbol '{symbol}' отсутствует в platform_profile_choices"
            )));
        }

        // 3. ровно один write.
        self.io.write(&self.profile_path, &format!("{symbol}\n"))?;

        // 4. fresh read-back current.
        let current = self.read_trimmed(&self.profile_path, "platform_profile")?;

        // 5. success только при совпадении.
        if current != symbol {
            return Err(ProviderError::BackendUnavailable(format!(
                "hardwared: read-back не подтвердил requested profile: expected='{symbol}', got='{current}'"
            )));
        }

        Ok(ApplyResult::Applied)
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

/// Ошибка авторизации.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorizeError {
    /// Не авторизован (deny/challenge/cancel) — клиенту `AccessDenied`.
    Denied(String),
    /// Ошибка проверки авторизации (polkit/transport) — клиенту `Failed`.
    Failed(String),
}

/// Узкая абстракция авторизации.
///
/// Production реализация — [`PolkitAuthorizer`]; тесты — fake authorizer.
/// Не доверяет UID/PID/profile из payload клиента: идентичность берётся из
/// unique sender входящего D-Bus message.
#[async_trait]
pub trait Authorizer: Send + Sync {
    /// Разрешить ли caller (unique sender name на system bus) операцию.
    async fn authorize(&self, sender: &str) -> Result<(), AuthorizeError>;
}

/// Production polkit authorizer.
///
/// - Subject: `system-bus-name` с unique sender из входящего D-Bus message;
/// - action: [`POLKIT_ACTION`];
/// - `AllowUserInteraction=false` (пустые флаги);
/// - разрешение только при `is_authorized=true`; challenge/deny/cancel → Denied.
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
                // AllowUserInteraction=false: пустые флаги (Default = empty).
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

/// Преобразовать `ProviderError` в D-Bus `fdo::Error` (детерминированный mapping).
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
    }
}

/// Порядок обработки `SetPerformanceProfile` (не зависит от zbus macro;
/// тестируемо с fake authorizer + fake writer):
///
/// 1. strict wire decode;
/// 2. authorization (zero backend reads/writes при отказе);
/// 3. writer `set_performance_profile` (ровно одна mutation + read-back);
/// 4. success → подтверждённый profile wire;
/// 5. error → соответствующий D-Bus error.
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
    // 1. strict wire decode — до любой авторизации/backend I/O.
    let profile = profile_from_wire(raw).map_err(provider_error_to_dbus)?;

    // 2. authorization — writer НЕ вызывается при отказе.
    authorizer.authorize(sender).await.map_err(|e| match e {
        AuthorizeError::Denied(msg) => zbus::fdo::Error::AccessDenied(msg),
        AuthorizeError::Failed(msg) => zbus::fdo::Error::Failed(msg),
    })?;

    // 3. ровно одна backend mutation + authoritative read-back.
    let result = writer
        .set_performance_profile(profile)
        .map_err(provider_error_to_dbus)?;

    // 4. success только при подтверждённом Applied.
    match result {
        ApplyResult::Applied => Ok(profile_to_wire(profile)),
        other => Err(zbus::fdo::Error::Failed(format!(
            "hardwared: операция не подтверждена: {other:?}"
        ))),
    }
}

/// Service object интерфейса `io.github.orbiscontrol.Hardware1`.
///
/// Не generic: writer — production с фиксированными kernel paths; authorizer —
/// trait object (production — polkit, тесты — fake).
pub struct HardwareService {
    authorizer: Box<dyn Authorizer>,
    writer: PlatformProfileWriter<StdProfileIo>,
}

impl HardwareService {
    /// Создать service object над авторизатором (writer — фиксированные paths).
    pub fn new(authorizer: Box<dyn Authorizer>) -> Self {
        Self {
            authorizer,
            writer: PlatformProfileWriter::default(),
        }
    }
}

#[zbus::interface(name = "io.github.orbiscontrol.Hardware1")]
impl HardwareService {
    /// Установить Performance profile (wire enum `y`; возвращает подтверждённый
    /// profile после writer read-back).
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
}

/// Client proxy контракта `io.github.orbiscontrol.Hardware1` (для sessiond).
///
/// Тот же wire contract, что server: `SetPerformanceProfile(y) -> y`; никаких
/// strings/paths. Константы shared (`DBUS_NAME`/`DBUS_OBJECT_PATH`).
#[zbus::proxy(
    interface = "io.github.orbiscontrol.Hardware1",
    default_service = "io.github.orbiscontrol.Hardware",
    default_path = "/io/github/orbiscontrol/Hardware"
)]
pub trait Hardware1 {
    /// Установить Performance profile; возвращает подтверждённый wire profile.
    fn set_performance_profile(&self, profile: u8) -> zbus::Result<u8>;
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// Fake backend: отдельные значения choices/profile, счётчики, опциональные
    /// сбои записи и подмена read-back.
    struct ScriptedIo {
        choices: Mutex<String>,
        profile: Mutex<String>,
        reads: AtomicUsize,
        writes: AtomicUsize,
        write_error: Mutex<Option<std::io::Error>>,
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
                // Клонировать io::Error нельзя; воспроизводим аналогичный.
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
    fn exact_symbol_mapping_is_total() {
        // Закрытая total mapping; никаких произвольных значений.
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
        // ровно один write; содержимое — exact symbol с trailing newline.
        assert_eq!(w.io.writes(), 1);
        assert_eq!(w.io.profile(), "quiet\n");
        // fresh reads: choices + read-back.
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
        io.set_override_read_back("performance"); // backend вернул другое
        let w = writer(io);
        let err = w
            .set_performance_profile(PerformanceProfile::Silent)
            .expect_err("read-back mismatch");
        assert!(matches!(err, ProviderError::BackendUnavailable(_)));
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
        // Backend изменился: quiet больше недоступен; второй вызов обязан
        // увидеть свежие choices, а не кэш.
        w.io.set_choices("balanced performance");
        w.io.clear_override_read_back();
        *w.io.profile.lock().unwrap() = "balanced".to_string();
        let err = w
            .set_performance_profile(PerformanceProfile::Silent)
            .expect_err("fresh choices без quiet");
        assert!(matches!(err, ProviderError::Unsupported(_)));
        // Второй вызов не выполнял write (свежая валидация).
        assert_eq!(w.io.writes(), 1);
    }

    // -----------------------------------------------------------------------
    // D-Bus boundary: wire decode, authorization order, error mapping
    // -----------------------------------------------------------------------

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
        assert_eq!(auth.calls(), 0, "decode выполняется до авторизации");
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
        io.set_override_read_back("performance"); // backend вернул другое
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
    }
}
