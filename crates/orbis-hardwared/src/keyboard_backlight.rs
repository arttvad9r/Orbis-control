//! Keyboard backlight brightness mutation backend.
//!
//! Authority: kernel LED class `/sys/class/leds/asus::kbd_backlight/`.
//!
//! Mutation path:
//! 1. Validate level <= max (fresh read from max_brightness);
//! 2. Write level to brightness;
//! 3. Fresh read-back brightness;
//! 4. Applied only if observed == requested.
//!
//! Никакого generic sysfs writer API.
//! Никакой asusd dependency — kernel LED class directly writable.

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use orbis_core::action::ApplyResult;
use orbis_providers::error::ProviderError;

use crate::{AuthorizeError, Authorizer, provider_error_to_dbus};

/// Kernel LED class path for keyboard backlight.
pub const KBD_BACKLIGHT_BRIGHTNESS_PATH: &str = "/sys/class/leds/asus::kbd_backlight/brightness";
/// Kernel LED class path for max brightness.
pub const KBD_BACKLIGHT_MAX_BRIGHTNESS_PATH: &str =
    "/sys/class/leds/asus::kbd_backlight/max_brightness";

/// Typed trait for keyboard backlight hardware operations.
#[async_trait]
pub trait KeyboardBacklightIo: Send + Sync {
    /// Read current brightness.
    async fn read_brightness(&self) -> Result<u32, ProviderError>;
    /// Read max brightness.
    async fn read_max_brightness(&self) -> Result<u32, ProviderError>;
    /// Write brightness (validated by caller).
    async fn write_brightness(&self, level: u32) -> Result<(), ProviderError>;
}

/// Read-only IO used only to prove structural keyboard backlight capability.
trait KeyboardBacklightProbeIo: Send + Sync {
    fn probe_read_brightness(&self) -> Result<u32, ProviderError>;
    fn probe_read_max_brightness(&self) -> Result<u32, ProviderError>;
}

/// Sysfs implementation of keyboard backlight IO.
pub struct SysfsKeyboardBacklightIo {
    brightness_path: PathBuf,
    max_path: PathBuf,
}

impl Default for SysfsKeyboardBacklightIo {
    fn default() -> Self {
        Self {
            brightness_path: PathBuf::from(KBD_BACKLIGHT_BRIGHTNESS_PATH),
            max_path: PathBuf::from(KBD_BACKLIGHT_MAX_BRIGHTNESS_PATH),
        }
    }
}

impl SysfsKeyboardBacklightIo {
    /// Создать с явными путями (test-only).
    pub fn new(brightness_path: PathBuf, max_path: PathBuf) -> Self {
        Self {
            brightness_path,
            max_path,
        }
    }
}

/// Map a read error on the LED path into the typed provider error.
fn map_led_error(path: &Path, error: std::io::Error) -> ProviderError {
    match error.kind() {
        std::io::ErrorKind::NotFound => ProviderError::Unsupported(format!(
            "asus kbd_backlight LED device absent: {}",
            path.display()
        )),
        std::io::ErrorKind::PermissionDenied => ProviderError::PermissionDenied(format!(
            "asus kbd_backlight write denied: {}",
            path.display()
        )),
        _ => ProviderError::Io(error),
    }
}

fn parse_led_value(raw: &str, what: &str) -> Result<u32, ProviderError> {
    raw.trim().parse::<u32>().map_err(|error| {
        ProviderError::Internal(format!("asus kbd_backlight: malformed {what}: {error}"))
    })
}

fn read_led_value(path: &Path, what: &str) -> Result<u32, ProviderError> {
    let raw = std::fs::read_to_string(path).map_err(|e| map_led_error(path, e))?;
    parse_led_value(&raw, what)
}

#[async_trait]
impl KeyboardBacklightIo for SysfsKeyboardBacklightIo {
    async fn read_brightness(&self) -> Result<u32, ProviderError> {
        read_led_value(&self.brightness_path, "brightness")
    }

    async fn read_max_brightness(&self) -> Result<u32, ProviderError> {
        read_led_value(&self.max_path, "max_brightness")
    }

    async fn write_brightness(&self, level: u32) -> Result<(), ProviderError> {
        std::fs::write(&self.brightness_path, format!("{level}\n"))
            .map_err(|e| map_led_error(&self.brightness_path, e))
    }
}

impl KeyboardBacklightProbeIo for SysfsKeyboardBacklightIo {
    fn probe_read_brightness(&self) -> Result<u32, ProviderError> {
        read_led_value(&self.brightness_path, "brightness")
    }

    fn probe_read_max_brightness(&self) -> Result<u32, ProviderError> {
        read_led_value(&self.max_path, "max_brightness")
    }
}

/// Typed mutation backend for keyboard backlight brightness.
#[async_trait]
pub trait KeyboardBacklightMutationBackend: Send + Sync {
    /// Set brightness with validation + authoritative read-back.
    async fn set_brightness(
        &self,
        level: u8,
    ) -> Result<KeyboardBacklightMutationReadback, ProviderError>;

    /// Typed mutation backend availability.
    ///
    /// `Supported` is structural evidence only: the LED ABI is readable,
    /// parseable and internally consistent. A later write can still fail.
    fn mutation_status(&self) -> KeyboardBacklightMutationStatus;
}

/// Fresh result of a keyboard backlight mutation and its authoritative read-back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyboardBacklightMutationReadback {
    /// Requested level.
    pub requested: u8,
    /// Observed level (fresh read-back).
    pub observed: u8,
    /// Applied only if observed == requested.
    pub result: ApplyResult,
}

/// Typed runtime evidence for keyboard backlight mutation availability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardBacklightMutationStatus {
    Supported,
    Unsupported,
    TemporarilyUnavailable,
    PermissionDenied,
    Unknown,
}

/// Stable wire values for `Hardware1.KeyboardBacklightMutationStatus`.
pub mod keyboard_backlight_mutation_wire {
    use super::KeyboardBacklightMutationStatus;

    pub const SUPPORTED: u8 = 0;
    pub const UNSUPPORTED: u8 = 1;
    pub const TEMPORARILY_UNAVAILABLE: u8 = 2;
    pub const PERMISSION_DENIED: u8 = 3;
    pub const UNKNOWN: u8 = 4;

    pub fn to_wire(status: KeyboardBacklightMutationStatus) -> u8 {
        match status {
            KeyboardBacklightMutationStatus::Supported => SUPPORTED,
            KeyboardBacklightMutationStatus::Unsupported => UNSUPPORTED,
            KeyboardBacklightMutationStatus::TemporarilyUnavailable => TEMPORARILY_UNAVAILABLE,
            KeyboardBacklightMutationStatus::PermissionDenied => PERMISSION_DENIED,
            KeyboardBacklightMutationStatus::Unknown => UNKNOWN,
        }
    }

    pub fn from_wire(raw: u8) -> Option<KeyboardBacklightMutationStatus> {
        match raw {
            SUPPORTED => Some(KeyboardBacklightMutationStatus::Supported),
            UNSUPPORTED => Some(KeyboardBacklightMutationStatus::Unsupported),
            TEMPORARILY_UNAVAILABLE => {
                Some(KeyboardBacklightMutationStatus::TemporarilyUnavailable)
            }
            PERMISSION_DENIED => Some(KeyboardBacklightMutationStatus::PermissionDenied),
            UNKNOWN => Some(KeyboardBacklightMutationStatus::Unknown),
            _ => None,
        }
    }
}

fn probe_error_status(what: &str, error: ProviderError) -> KeyboardBacklightMutationStatus {
    match error {
        ProviderError::Unsupported(_) => KeyboardBacklightMutationStatus::Unsupported,
        ProviderError::PermissionDenied(_) => KeyboardBacklightMutationStatus::PermissionDenied,
        ProviderError::Io(_) | ProviderError::BackendUnavailable(_) | ProviderError::Timeout(_) => {
            KeyboardBacklightMutationStatus::TemporarilyUnavailable
        }
        other => {
            tracing::warn!(
                field = what,
                error = %other,
                "keyboard backlight structural probe failed closed"
            );
            KeyboardBacklightMutationStatus::Unknown
        }
    }
}

fn probe_mutation_status(io: &dyn KeyboardBacklightProbeIo) -> KeyboardBacklightMutationStatus {
    let brightness = match io.probe_read_brightness() {
        Ok(value) => value,
        Err(error) => return probe_error_status("brightness", error),
    };
    let max = match io.probe_read_max_brightness() {
        Ok(value) => value,
        Err(error) => return probe_error_status("max_brightness", error),
    };

    if max == 0 {
        tracing::warn!(
            brightness,
            max_brightness = max,
            "keyboard backlight structural probe found zero max_brightness"
        );
        return KeyboardBacklightMutationStatus::Unknown;
    }
    if brightness > max {
        tracing::warn!(
            brightness,
            max_brightness = max,
            "keyboard backlight structural probe found inconsistent LED ABI"
        );
        return KeyboardBacklightMutationStatus::Unknown;
    }

    KeyboardBacklightMutationStatus::Supported
}

/// Sysfs-based keyboard backlight mutation backend.
#[derive(Default)]
pub struct SysfsKeyboardBacklightMutationBackend {
    io: SysfsKeyboardBacklightIo,
}

const READBACK_SETTLE_TIMEOUT: Duration = Duration::from_millis(250);
const READBACK_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Confirm a single already-dispatched brightness write while the LED driver
/// settles. This polls only the authoritative read path; it never retries the
/// mutation itself.
async fn read_brightness_until_match(
    io: &dyn KeyboardBacklightIo,
    expected: u32,
) -> Result<u32, ProviderError> {
    let deadline = Instant::now() + READBACK_SETTLE_TIMEOUT;
    let mut observed = io.read_brightness().await?;
    while observed != expected {
        if Instant::now() >= deadline {
            return Err(ProviderError::BackendUnavailable(format!(
                "asus kbd_backlight read-back mismatch: expected={expected}, got={observed}"
            )));
        }
        // Hardware1 methods are also dispatched by zbus' executor thread,
        // which is not guaranteed to have a Tokio reactor. The interval is
        // tightly bounded and only delays a read-back poll; it never retries
        // the already-dispatched mutation.
        std::thread::sleep(READBACK_POLL_INTERVAL);
        observed = io.read_brightness().await?;
    }
    Ok(observed)
}

impl SysfsKeyboardBacklightMutationBackend {
    pub fn new(io: SysfsKeyboardBacklightIo) -> Self {
        Self { io }
    }
}

#[async_trait]
impl KeyboardBacklightMutationBackend for SysfsKeyboardBacklightMutationBackend {
    async fn set_brightness(
        &self,
        level: u8,
    ) -> Result<KeyboardBacklightMutationReadback, ProviderError> {
        let max = self.io.read_max_brightness().await?;
        if level as u32 > max {
            return Err(ProviderError::InvalidRequest(format!(
                "asus kbd_backlight: level ({level}) > max ({max})"
            )));
        }

        self.io.write_brightness(level as u32).await?;

        let observed = read_brightness_until_match(&self.io, level as u32).await?;

        Ok(KeyboardBacklightMutationReadback {
            requested: level,
            observed: observed as u8,
            result: ApplyResult::Applied,
        })
    }

    fn mutation_status(&self) -> KeyboardBacklightMutationStatus {
        probe_mutation_status(&self.io)
    }
}

/// Обработка keyboard backlight mutation до публичного D-Bus boundary.
pub async fn handle_set_keyboard_backlight(
    authorizer: &dyn Authorizer,
    backend: &dyn KeyboardBacklightMutationBackend,
    level: u8,
    sender: &str,
) -> zbus::fdo::Result<u8> {
    tracing::info!(level, "keyboard_backlight Hardware1 request");
    authorizer.authorize(sender).await.map_err(|e| match e {
        AuthorizeError::Denied(msg) => zbus::fdo::Error::AccessDenied(msg),
        AuthorizeError::Failed(msg) => zbus::fdo::Error::Failed(msg),
    })?;

    let readback = backend
        .set_brightness(level)
        .await
        .map_err(provider_error_to_dbus)?;
    match readback.result {
        ApplyResult::Applied => Ok(readback.observed),
        other => Err(zbus::fdo::Error::Failed(format!(
            "hardwared: keyboard backlight operation not confirmed: {other:?}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use super::*;
    use crate::{AuthorizeError, Authorizer};

    struct TestBackend {
        brightness: Arc<Mutex<u32>>,
        max: u32,
        writes: Arc<AtomicUsize>,
        read_errors: Arc<Mutex<Option<String>>>,
        write_errors: Arc<Mutex<Option<String>>>,
    }

    impl TestBackend {
        fn new(brightness: u32, max: u32) -> Self {
            Self {
                brightness: Arc::new(Mutex::new(brightness)),
                max,
                writes: Arc::new(AtomicUsize::new(0)),
                read_errors: Arc::new(Mutex::new(None)),
                write_errors: Arc::new(Mutex::new(None)),
            }
        }

        fn writes(&self) -> usize {
            self.writes.load(Ordering::SeqCst)
        }

        fn set_write_error(&self, err: &str) {
            *self.write_errors.lock().unwrap() = Some(err.to_string());
        }

        fn set_read_error(&self, err: &str) {
            *self.read_errors.lock().unwrap() = Some(err.to_string());
        }
    }

    #[async_trait]
    impl KeyboardBacklightMutationBackend for TestBackend {
        async fn set_brightness(
            &self,
            level: u8,
        ) -> Result<KeyboardBacklightMutationReadback, ProviderError> {
            // Validation: level <= max
            if level as u32 > self.max {
                return Err(ProviderError::InvalidRequest(format!(
                    "level ({level}) > max ({})",
                    self.max
                )));
            }

            // Write
            self.writes.fetch_add(1, Ordering::SeqCst);
            if let Some(e) = self.write_errors.lock().unwrap().clone() {
                return Err(ProviderError::Dbus(e));
            }
            *self.brightness.lock().unwrap() = level as u32;

            // Read-back
            if let Some(e) = self.read_errors.lock().unwrap().clone() {
                return Err(ProviderError::Dbus(e));
            }
            let observed = *self.brightness.lock().unwrap();
            if observed != level as u32 {
                return Err(ProviderError::BackendUnavailable(format!(
                    "readback mismatch: expected={level}, got={observed}"
                )));
            }

            Ok(KeyboardBacklightMutationReadback {
                requested: level,
                observed: observed as u8,
                result: ApplyResult::Applied,
            })
        }

        fn mutation_status(&self) -> KeyboardBacklightMutationStatus {
            KeyboardBacklightMutationStatus::Supported
        }
    }

    #[derive(Clone, Copy)]
    enum ProbeRead {
        Value(u32),
        Missing,
        PermissionDenied,
        Transient,
        Malformed,
    }

    impl ProbeRead {
        fn into_result(self, what: &str) -> Result<u32, ProviderError> {
            match self {
                Self::Value(value) => Ok(value),
                Self::Missing => Err(map_led_error(
                    Path::new("/test/kbd_backlight"),
                    std::io::Error::from(std::io::ErrorKind::NotFound),
                )),
                Self::PermissionDenied => Err(map_led_error(
                    Path::new("/test/kbd_backlight"),
                    std::io::Error::from(std::io::ErrorKind::PermissionDenied),
                )),
                Self::Transient => Err(map_led_error(
                    Path::new("/test/kbd_backlight"),
                    std::io::Error::from(std::io::ErrorKind::WouldBlock),
                )),
                Self::Malformed => parse_led_value("not-a-number", what),
            }
        }
    }

    struct ProbeIo {
        brightness: ProbeRead,
        max: ProbeRead,
        writes: Arc<AtomicUsize>,
    }

    impl ProbeIo {
        fn new(brightness: ProbeRead, max: ProbeRead) -> Self {
            Self {
                brightness,
                max,
                writes: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn writes(&self) -> usize {
            self.writes.load(Ordering::SeqCst)
        }
    }

    impl KeyboardBacklightProbeIo for ProbeIo {
        fn probe_read_brightness(&self) -> Result<u32, ProviderError> {
            self.brightness.into_result("brightness")
        }

        fn probe_read_max_brightness(&self) -> Result<u32, ProviderError> {
            self.max.into_result("max_brightness")
        }
    }

    #[async_trait]
    impl KeyboardBacklightIo for ProbeIo {
        async fn read_brightness(&self) -> Result<u32, ProviderError> {
            self.brightness.into_result("brightness")
        }

        async fn read_max_brightness(&self) -> Result<u32, ProviderError> {
            self.max.into_result("max_brightness")
        }

        async fn write_brightness(&self, _level: u32) -> Result<(), ProviderError> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
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
    }

    impl FakeAuthorizer {
        fn new(outcome: AuthOutcome) -> Self {
            Self {
                outcome: Arc::new(Mutex::new(outcome)),
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }
        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl Authorizer for FakeAuthorizer {
        async fn authorize(&self, _sender: &str) -> Result<(), AuthorizeError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match *self.outcome.lock().unwrap() {
                AuthOutcome::Ok => Ok(()),
                AuthOutcome::Denied => Err(AuthorizeError::Denied("denied".into())),
                AuthOutcome::Failed => Err(AuthorizeError::Failed("polkit down".into())),
            }
        }
    }

    #[test]
    fn probe_valid_brightness_and_max_is_supported() {
        let io = ProbeIo::new(ProbeRead::Value(2), ProbeRead::Value(3));
        assert_eq!(
            probe_mutation_status(&io),
            KeyboardBacklightMutationStatus::Supported
        );
    }

    #[test]
    fn probe_missing_brightness_or_max_is_unsupported() {
        for (brightness, max) in [
            (ProbeRead::Missing, ProbeRead::Value(3)),
            (ProbeRead::Value(1), ProbeRead::Missing),
        ] {
            let io = ProbeIo::new(brightness, max);
            assert_eq!(
                probe_mutation_status(&io),
                KeyboardBacklightMutationStatus::Unsupported
            );
        }
    }

    #[test]
    fn probe_permission_error_is_preserved() {
        let io = ProbeIo::new(ProbeRead::PermissionDenied, ProbeRead::Value(3));
        assert_eq!(
            probe_mutation_status(&io),
            KeyboardBacklightMutationStatus::PermissionDenied
        );
    }

    #[test]
    fn probe_transient_io_is_temporarily_unavailable() {
        let io = ProbeIo::new(ProbeRead::Transient, ProbeRead::Value(3));
        assert_eq!(
            probe_mutation_status(&io),
            KeyboardBacklightMutationStatus::TemporarilyUnavailable
        );
    }

    #[test]
    fn probe_malformed_values_fail_closed() {
        for (brightness, max) in [
            (ProbeRead::Malformed, ProbeRead::Value(3)),
            (ProbeRead::Value(1), ProbeRead::Malformed),
        ] {
            let io = ProbeIo::new(brightness, max);
            assert_eq!(
                probe_mutation_status(&io),
                KeyboardBacklightMutationStatus::Unknown
            );
        }
    }

    #[test]
    fn probe_brightness_above_max_is_rejected() {
        let io = ProbeIo::new(ProbeRead::Value(4), ProbeRead::Value(3));
        assert_eq!(
            probe_mutation_status(&io),
            KeyboardBacklightMutationStatus::Unknown
        );
    }

    #[test]
    fn probe_zero_max_is_rejected() {
        let io = ProbeIo::new(ProbeRead::Value(0), ProbeRead::Value(0));
        assert_eq!(
            probe_mutation_status(&io),
            KeyboardBacklightMutationStatus::Unknown
        );
    }

    #[test]
    fn probe_never_calls_write() {
        let io = ProbeIo::new(ProbeRead::Value(1), ProbeRead::Value(3));
        assert_eq!(
            probe_mutation_status(&io),
            KeyboardBacklightMutationStatus::Supported
        );
        assert_eq!(io.writes(), 0);
    }

    #[tokio::test]
    async fn set_zero_level_applied() {
        let tb = TestBackend::new(0, 3);
        let result = tb.set_brightness(0).await.unwrap();
        assert_eq!(result.requested, 0);
        assert_eq!(result.observed, 0);
        assert_eq!(result.result, ApplyResult::Applied);
        assert_eq!(tb.writes(), 1);
    }

    #[tokio::test]
    async fn set_max_level_applied() {
        let tb = TestBackend::new(0, 3);
        let result = tb.set_brightness(3).await.unwrap();
        assert_eq!(result.requested, 3);
        assert_eq!(result.observed, 3);
        assert_eq!(result.result, ApplyResult::Applied);
    }

    struct DelayedReadbackIo {
        reads: Mutex<VecDeque<u32>>,
    }

    #[async_trait]
    impl KeyboardBacklightIo for DelayedReadbackIo {
        async fn read_brightness(&self) -> Result<u32, ProviderError> {
            Ok(self.reads.lock().unwrap().pop_front().unwrap_or(3))
        }

        async fn read_max_brightness(&self) -> Result<u32, ProviderError> {
            Ok(3)
        }

        async fn write_brightness(&self, _level: u32) -> Result<(), ProviderError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn delayed_readback_is_confirmed_without_second_write() {
        let io = DelayedReadbackIo {
            reads: Mutex::new(VecDeque::from([0, 3])),
        };

        let observed = read_brightness_until_match(&io, 3).await.unwrap();

        assert_eq!(observed, 3);
    }

    #[tokio::test]
    async fn requested_exceeding_max_rejected() {
        let tb = TestBackend::new(0, 3);
        let result = tb.set_brightness(5).await;
        assert!(matches!(result, Err(ProviderError::InvalidRequest(_))));
        assert_eq!(tb.writes(), 0);
    }

    #[tokio::test]
    async fn write_failure_propagates() {
        let tb = TestBackend::new(0, 3);
        tb.set_write_error("io error");
        let result = tb.set_brightness(2).await;
        assert!(matches!(result, Err(ProviderError::Dbus(_))));
        assert_eq!(tb.writes(), 1);
    }

    #[tokio::test]
    async fn readback_failure_propagates() {
        let tb = TestBackend::new(0, 3);
        tb.set_read_error("read error");
        let result = tb.set_brightness(2).await;
        assert!(matches!(result, Err(ProviderError::Dbus(_))));
    }

    #[tokio::test]
    async fn readback_mismatch_is_not_applied() {
        let tb = TestBackend::new(0, 3);
        // Simulate readback returning different value by using read_error
        tb.set_read_error("mismatch");
        let result = tb.set_brightness(2).await;
        assert!(matches!(result, Err(ProviderError::Dbus(_))));
    }

    #[tokio::test]
    async fn auth_denied_zero_writes() {
        let tb = TestBackend::new(0, 3);
        let auth = FakeAuthorizer::new(AuthOutcome::Denied);
        let err = handle_set_keyboard_backlight(&auth, &tb, 2, ":1.42")
            .await
            .expect_err("denied");
        assert!(matches!(err, zbus::fdo::Error::AccessDenied(_)));
        assert_eq!(tb.writes(), 0);
        assert_eq!(auth.calls(), 1);
    }

    #[tokio::test]
    async fn auth_failed_zero_writes() {
        let tb = TestBackend::new(0, 3);
        let auth = FakeAuthorizer::new(AuthOutcome::Failed);
        let err = handle_set_keyboard_backlight(&auth, &tb, 2, ":1.7")
            .await
            .expect_err("polkit down");
        assert!(matches!(err, zbus::fdo::Error::Failed(_)));
        assert_eq!(tb.writes(), 0);
    }

    #[tokio::test]
    async fn auth_ok_applies_and_returns_observed() {
        let tb = TestBackend::new(0, 3);
        let auth = FakeAuthorizer::new(AuthOutcome::Ok);
        let observed = handle_set_keyboard_backlight(&auth, &tb, 2, ":1.42")
            .await
            .expect("authorized");
        assert_eq!(observed, 2);
        assert_eq!(tb.writes(), 1);
    }

    #[tokio::test]
    async fn mutation_status_wire_roundtrip() {
        for status in [
            KeyboardBacklightMutationStatus::Supported,
            KeyboardBacklightMutationStatus::Unsupported,
            KeyboardBacklightMutationStatus::TemporarilyUnavailable,
            KeyboardBacklightMutationStatus::PermissionDenied,
            KeyboardBacklightMutationStatus::Unknown,
        ] {
            let wire = keyboard_backlight_mutation_wire::to_wire(status);
            assert_eq!(
                keyboard_backlight_mutation_wire::from_wire(wire),
                Some(status)
            );
        }
        assert_eq!(keyboard_backlight_mutation_wire::from_wire(99), None);
    }

    #[test]
    fn production_paths_are_fixed() {
        assert_eq!(
            KBD_BACKLIGHT_BRIGHTNESS_PATH,
            "/sys/class/leds/asus::kbd_backlight/brightness"
        );
        assert_eq!(
            KBD_BACKLIGHT_MAX_BRIGHTNESS_PATH,
            "/sys/class/leds/asus::kbd_backlight/max_brightness"
        );
    }
}
