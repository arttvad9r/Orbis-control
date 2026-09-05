//! Typed ASUS firmware-attribute mutation backends.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use orbis_core::action::ApplyResult;
use orbis_core::firmware::BootSoundState;
use orbis_providers::error::ProviderError;

use crate::{AuthorizeError, Authorizer, provider_error_to_dbus};

/// Fixed kernel ABI path for the ASUS POST sound attribute.
pub const BOOT_SOUND_CURRENT_VALUE_PATH: &str =
    "/sys/class/firmware-attributes/asus-armoury/attributes/boot_sound/current_value";

/// Narrow IO contract for the ASUS POST sound attribute.
#[async_trait]
pub trait BootSoundIo: Send + Sync {
    /// Read the current boolean wire value.
    async fn read(&self) -> Result<BootSoundState, ProviderError>;
    /// Write one validated boolean wire value.
    async fn write(&self, state: BootSoundState) -> Result<(), ProviderError>;
}

/// Production implementation over the fixed kernel firmware-attributes ABI.
pub struct SysfsBootSoundIo {
    path: PathBuf,
}

impl Default for SysfsBootSoundIo {
    fn default() -> Self {
        Self {
            path: PathBuf::from(BOOT_SOUND_CURRENT_VALUE_PATH),
        }
    }
}

impl SysfsBootSoundIo {
    /// Construct an IO object for a test fixture.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

fn map_error(path: &Path, error: std::io::Error) -> ProviderError {
    match error.kind() {
        std::io::ErrorKind::NotFound => ProviderError::Unsupported(format!(
            "asus-armoury boot_sound attribute absent: {}",
            path.display()
        )),
        std::io::ErrorKind::PermissionDenied => ProviderError::PermissionDenied(format!(
            "asus-armoury boot_sound write denied: {}",
            path.display()
        )),
        _ => ProviderError::Io(error),
    }
}

fn parse(raw: &str) -> Result<BootSoundState, ProviderError> {
    let value = raw.trim().parse::<u32>().map_err(|error| {
        ProviderError::Internal(format!("asus-armoury boot_sound malformed value: {error}"))
    })?;
    BootSoundState::from_kernel_value(value).ok_or_else(|| {
        ProviderError::Internal(format!(
            "asus-armoury boot_sound value outside boolean ABI: {value}"
        ))
    })
}

#[async_trait]
impl BootSoundIo for SysfsBootSoundIo {
    async fn read(&self) -> Result<BootSoundState, ProviderError> {
        let raw =
            std::fs::read_to_string(&self.path).map_err(|error| map_error(&self.path, error))?;
        parse(&raw)
    }

    async fn write(&self, state: BootSoundState) -> Result<(), ProviderError> {
        std::fs::write(&self.path, format!("{}\n", state.kernel_value()))
            .map_err(|error| map_error(&self.path, error))
    }
}

/// Fresh result of a POST sound mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootSoundMutationReadback {
    /// Requested state.
    pub requested: BootSoundState,
    /// Authoritative state observed after one write.
    pub observed: BootSoundState,
    /// Applied only when requested and observed match.
    pub result: ApplyResult,
}

/// Runtime evidence for the typed POST sound writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootSoundMutationStatus {
    /// The fixed ABI is readable and structurally valid.
    Supported,
    /// The fixed ABI is absent.
    Unsupported,
    /// The ABI exists but is temporarily unavailable.
    TemporarilyUnavailable,
    /// Current permissions prevent access.
    PermissionDenied,
    /// Evidence was malformed or inconclusive.
    Unknown,
}

/// Stable `Hardware1` wire values for [`BootSoundMutationStatus`].
pub mod boot_sound_mutation_wire {
    use super::BootSoundMutationStatus;

    pub const SUPPORTED: u8 = 0;
    pub const UNSUPPORTED: u8 = 1;
    pub const TEMPORARILY_UNAVAILABLE: u8 = 2;
    pub const PERMISSION_DENIED: u8 = 3;
    pub const UNKNOWN: u8 = 4;

    pub fn to_wire(status: BootSoundMutationStatus) -> u8 {
        match status {
            BootSoundMutationStatus::Supported => SUPPORTED,
            BootSoundMutationStatus::Unsupported => UNSUPPORTED,
            BootSoundMutationStatus::TemporarilyUnavailable => TEMPORARILY_UNAVAILABLE,
            BootSoundMutationStatus::PermissionDenied => PERMISSION_DENIED,
            BootSoundMutationStatus::Unknown => UNKNOWN,
        }
    }
}

/// Typed POST sound mutation backend.
pub struct BootSoundMutationBackend<I> {
    io: I,
}

impl<I> BootSoundMutationBackend<I> {
    /// Construct without performing I/O.
    pub fn new(io: I) -> Self {
        Self { io }
    }
}

impl<I> BootSoundMutationBackend<I>
where
    I: BootSoundIo,
{
    /// Perform one write followed by one authoritative read-back.
    pub async fn set_boot_sound(
        &self,
        enabled: bool,
    ) -> Result<BootSoundMutationReadback, ProviderError> {
        let requested = if enabled {
            BootSoundState::Enabled
        } else {
            BootSoundState::Disabled
        };
        self.io.write(requested).await?;
        let observed = self.io.read().await?;
        if observed != requested {
            return Err(ProviderError::BackendUnavailable(format!(
                "asus-armoury boot_sound read-back mismatch: expected={requested:?}, got={observed:?}"
            )));
        }
        Ok(BootSoundMutationReadback {
            requested,
            observed,
            result: ApplyResult::Applied,
        })
    }

    /// Probe only the fixed read path; never write during discovery.
    pub async fn mutation_status(&self) -> BootSoundMutationStatus {
        match self.io.read().await {
            Ok(_) => BootSoundMutationStatus::Supported,
            Err(ProviderError::Unsupported(_)) => BootSoundMutationStatus::Unsupported,
            Err(ProviderError::PermissionDenied(_)) => BootSoundMutationStatus::PermissionDenied,
            Err(ProviderError::Io(_)) | Err(ProviderError::Timeout(_)) => {
                BootSoundMutationStatus::TemporarilyUnavailable
            }
            Err(_) => BootSoundMutationStatus::Unknown,
        }
    }
}

/// Handle a typed `Hardware1.SetBootSound` request.
pub async fn handle_set_boot_sound(
    authorizer: &dyn Authorizer,
    backend: &dyn BootSoundMutationOperation,
    enabled: bool,
    sender: &str,
) -> zbus::fdo::Result<u8> {
    authorizer
        .authorize(sender)
        .await
        .map_err(|error| match error {
            AuthorizeError::Denied(message) => zbus::fdo::Error::AccessDenied(message),
            AuthorizeError::Failed(message) => zbus::fdo::Error::Failed(message),
        })?;
    let readback = backend
        .set_boot_sound(enabled)
        .await
        .map_err(provider_error_to_dbus)?;
    match readback.result {
        ApplyResult::Applied => Ok(readback.observed.kernel_value() as u8),
        other => Err(zbus::fdo::Error::Failed(format!(
            "hardwared: boot sound operation not confirmed: {other:?}"
        ))),
    }
}

/// Erased typed operation used by the D-Bus boundary and private tests.
#[async_trait]
pub trait BootSoundMutationOperation: Send + Sync {
    /// Apply one POST sound request with read-back.
    async fn set_boot_sound(
        &self,
        enabled: bool,
    ) -> Result<BootSoundMutationReadback, ProviderError>;
    /// Return read-only runtime capability evidence.
    async fn mutation_status(&self) -> BootSoundMutationStatus;
}

#[async_trait]
impl<I> BootSoundMutationOperation for BootSoundMutationBackend<I>
where
    I: BootSoundIo,
{
    async fn set_boot_sound(
        &self,
        enabled: bool,
    ) -> Result<BootSoundMutationReadback, ProviderError> {
        self.set_boot_sound(enabled).await
    }

    async fn mutation_status(&self) -> BootSoundMutationStatus {
        self.mutation_status().await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;

    struct FakeIo {
        state: Arc<Mutex<BootSoundState>>,
        writes: Arc<AtomicUsize>,
        read_override: Option<BootSoundState>,
    }

    #[async_trait]
    impl BootSoundIo for FakeIo {
        async fn read(&self) -> Result<BootSoundState, ProviderError> {
            Ok(self.read_override.unwrap_or(*self.state.lock().unwrap()))
        }

        async fn write(&self, state: BootSoundState) -> Result<(), ProviderError> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            *self.state.lock().unwrap() = state;
            Ok(())
        }
    }

    struct AuthDenied;

    #[async_trait]
    impl Authorizer for AuthDenied {
        async fn authorize(&self, _: &str) -> Result<(), AuthorizeError> {
            Err(AuthorizeError::Denied("denied".into()))
        }
    }

    #[tokio::test]
    async fn one_write_and_matching_readback_is_applied() {
        let io = FakeIo {
            state: Arc::new(Mutex::new(BootSoundState::Disabled)),
            writes: Arc::new(AtomicUsize::new(0)),
            read_override: None,
        };
        let result = BootSoundMutationBackend::new(io)
            .set_boot_sound(true)
            .await
            .unwrap();
        assert_eq!(result.observed, BootSoundState::Enabled);
        assert_eq!(result.result, ApplyResult::Applied);
    }

    #[tokio::test]
    async fn mismatched_readback_is_not_success() {
        let io = FakeIo {
            state: Arc::new(Mutex::new(BootSoundState::Disabled)),
            writes: Arc::new(AtomicUsize::new(0)),
            read_override: Some(BootSoundState::Disabled),
        };
        assert!(matches!(
            BootSoundMutationBackend::new(io).set_boot_sound(true).await,
            Err(ProviderError::BackendUnavailable(_))
        ));
    }

    #[tokio::test]
    async fn authorization_denial_prevents_write() {
        let io = FakeIo {
            state: Arc::new(Mutex::new(BootSoundState::Disabled)),
            writes: Arc::new(AtomicUsize::new(0)),
            read_override: None,
        };
        let writes = io.writes.clone();
        let backend = BootSoundMutationBackend::new(io);
        let result = handle_set_boot_sound(&AuthDenied, &backend, true, ":1.2").await;
        assert!(matches!(result, Err(zbus::fdo::Error::AccessDenied(_))));
        assert_eq!(writes.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn production_path_is_fixed() {
        assert_eq!(
            BOOT_SOUND_CURRENT_VALUE_PATH,
            "/sys/class/firmware-attributes/asus-armoury/attributes/boot_sound/current_value"
        );
    }
}
