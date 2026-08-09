//! Read-only kernel ASUS Armoury firmware-attributes backend для GPU MUX и
//! dGPU access policy.
//!
//! Реализует `GpuMuxProvider` и `GpuAccessProvider` через read-only sysfs
//! `/sys/class/firmware-attributes/asus-armoury/attributes/.../current_value`.
//!
//! - пути к sysfs передаются в source извне; source не открывает файлы на
//!   запись, не выполняет hardware mutation и не создаёт runtime;
//! - каждый вызов `mux_state()`/`access_policy()` выполняет новый authoritative
//!   read (кэш отсутствует);
//! - mapping PROVEN из kernel 7.1.7 `drivers/platform/x86/asus-armoury.c`:
//!   `gpu_mux_mode`: 0 → Discrete, 1 → Integrated;
//!   `dgpu_disable`: 0 → Unblocked, 1 → Blocked;
//! - неизвестный future numeric value → соответствующий `Unknown`;
//! - malformed value → `ProviderError::Internal`;
//! - I/O error → `ProviderError::Io`;
//! - никаких queued/pending reads, никаких writes.

use std::time::Duration;

use async_trait::async_trait;
use orbis_core::diagnostics::DiagnosticEntry;
use orbis_core::gpu::{GpuAccessPolicy, GpuMuxState};
use orbis_core::identity::BackendIdentity;
use orbis_providers::error::ProviderError;
use orbis_providers::traits::{GpuAccessProvider, GpuMuxProvider, Provider, ProviderHealth};

/// Raw snapshot kernel ASUS Armoury current values (независим от domain).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArmouryGpuSnapshot {
    /// Текущее значение `gpu_mux_mode` (0/1).
    pub mux_mode_raw: Option<u32>,
    /// Текущее значение `dgpu_disable` (0/1).
    pub dgpu_disable_raw: Option<u32>,
}

/// Testable источник kernel ASUS Armoury current values.
#[async_trait]
pub trait ArmouryGpuSource: Send + Sync {
    /// Прочитать snapshot (authoritative, без кэша).
    async fn read_snapshot(&self) -> Result<ArmouryGpuSnapshot, ProviderError>;
}

/// Реальный sysfs источник.
///
/// Хранит пути к `gpu_mux_mode/current_value` и `dgpu_disable/current_value`;
/// I/O начинается только в `read_snapshot().await`. Конструктор не выполняет
/// I/O, не проверяет существование файлов и не открывает их на запись.
pub struct SysfsArmouryGpuSource {
    mux_path: std::path::PathBuf,
    dgpu_path: std::path::PathBuf,
}

impl SysfsArmouryGpuSource {
    /// Создать источник с явными путями; по умолчанию — стандартные ABI paths.
    pub fn new(mux_path: std::path::PathBuf, dgpu_path: std::path::PathBuf) -> Self {
        Self {
            mux_path,
            dgpu_path,
        }
    }
}

impl Default for SysfsArmouryGpuSource {
    fn default() -> Self {
        Self::new(
            std::path::PathBuf::from(
                "/sys/class/firmware-attributes/asus-armoury/attributes/gpu_mux_mode/current_value",
            ),
            std::path::PathBuf::from(
                "/sys/class/firmware-attributes/asus-armoury/attributes/dgpu_disable/current_value",
            ),
        )
    }
}

impl SysfsArmouryGpuSource {
    /// Прочитать один файл, trim, распарсить u32.
    ///
    /// Отсутствие файла → `None` (capability может отсутствовать на других
    /// системах); malformed содержимое → `Internal`; I/O ошибка → `Io`.
    fn read_u32(path: &std::path::Path) -> Result<Option<u32>, ProviderError> {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(ProviderError::Io(e)),
        };
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        trimmed.parse::<u32>().map(Some).map_err(|_| {
            ProviderError::Internal(format!(
                "kernel asus-armoury: невалидное значение '{}' в '{}'",
                trimmed,
                path.display()
            ))
        })
    }
}

#[async_trait]
impl ArmouryGpuSource for SysfsArmouryGpuSource {
    async fn read_snapshot(&self) -> Result<ArmouryGpuSnapshot, ProviderError> {
        // Простое read-only чтение маленьких файлов выполняется синхронно в
        // async context: операции tiny и не блокируют runtime заметно.
        Ok(ArmouryGpuSnapshot {
            mux_mode_raw: Self::read_u32(&self.mux_path)?,
            dgpu_disable_raw: Self::read_u32(&self.dgpu_path)?,
        })
    }
}

/// Read-only provider для MUX + dGPU access policy над kernel Armoury source.
///
/// Реализует оба независимых capability traits и НЕ реализует legacy
/// `GpuProvider`.
pub struct ArmouryGpuProvider<S> {
    source: S,
}

impl<S> ArmouryGpuProvider<S> {
    /// Создать provider над source.
    ///
    /// Не открывает sysfs и не выполняет I/O.
    pub fn new(source: S) -> Self {
        Self { source }
    }
}

impl<S> Provider for ArmouryGpuProvider<S>
where
    S: ArmouryGpuSource,
{
    fn id(&self) -> &'static str {
        "asus-armoury-gpu"
    }

    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("asus-armoury-gpu")
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }

    fn explain_unsupported(&self, feature: &str) -> String {
        format!("kernel asus-armoury read-only backend: функция '{feature}' недоступна")
    }

    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }

    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        vec![DiagnosticEntry::new(
            "provider.asus-armoury-gpu",
            "read-only kernel asus-armoury GPU backend (без hardware writes)",
        )]
    }
}

/// Преобразовать raw `gpu_mux_mode` value по PROVEN kernel mapping.
fn mux_from_raw(raw: u32) -> GpuMuxState {
    match raw {
        0 => GpuMuxState::Discrete,
        1 => GpuMuxState::Integrated,
        _ => GpuMuxState::Unknown,
    }
}

/// Преобразовать raw `dgpu_disable` value по PROVEN kernel mapping.
fn access_from_raw(raw: u32) -> GpuAccessPolicy {
    match raw {
        0 => GpuAccessPolicy::Unblocked,
        1 => GpuAccessPolicy::Blocked,
        _ => GpuAccessPolicy::Unknown,
    }
}

#[async_trait]
impl<S> GpuMuxProvider for ArmouryGpuProvider<S>
where
    S: ArmouryGpuSource,
{
    async fn mux_state(&self) -> Result<GpuMuxState, ProviderError> {
        let snapshot = self.source.read_snapshot().await?;
        Ok(snapshot
            .mux_mode_raw
            .map(mux_from_raw)
            .unwrap_or(GpuMuxState::Unknown))
    }
}

#[async_trait]
impl<S> GpuAccessProvider for ArmouryGpuProvider<S>
where
    S: ArmouryGpuSource,
{
    async fn access_policy(&self) -> Result<GpuAccessPolicy, ProviderError> {
        let snapshot = self.source.read_snapshot().await?;
        Ok(snapshot
            .dgpu_disable_raw
            .map(access_from_raw)
            .unwrap_or(GpuAccessPolicy::Unknown))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// Тестовый источник: очередь snapshots, счётчик вызовов.
    struct ScriptedSource {
        snapshots:
            std::sync::Mutex<std::collections::VecDeque<Result<ArmouryGpuSnapshot, ProviderError>>>,
        reads: AtomicUsize,
    }

    impl ScriptedSource {
        fn new(snapshots: Vec<Result<ArmouryGpuSnapshot, ProviderError>>) -> Self {
            Self {
                snapshots: std::sync::Mutex::new(snapshots.into()),
                reads: AtomicUsize::new(0),
            }
        }

        fn reads(&self) -> usize {
            self.reads.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl ArmouryGpuSource for ScriptedSource {
        async fn read_snapshot(&self) -> Result<ArmouryGpuSnapshot, ProviderError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            self.snapshots.lock().unwrap().pop_front().ok_or_else(|| {
                ProviderError::Internal("scripted source: очередь исчерпана".into())
            })?
        }
    }

    fn snapshot(mux: Option<u32>, dgpu: Option<u32>) -> ArmouryGpuSnapshot {
        ArmouryGpuSnapshot {
            mux_mode_raw: mux,
            dgpu_disable_raw: dgpu,
        }
    }

    fn provider(
        snapshots: Vec<Result<ArmouryGpuSnapshot, ProviderError>>,
    ) -> ArmouryGpuProvider<ScriptedSource> {
        ArmouryGpuProvider::new(ScriptedSource::new(snapshots))
    }

    #[tokio::test]
    async fn maps_proven_mux_values() {
        let cases = [(0, GpuMuxState::Discrete), (1, GpuMuxState::Integrated)];
        for (raw, expected) in cases {
            let p = provider(vec![Ok(snapshot(Some(raw), Some(0)))]);
            assert_eq!(p.mux_state().await.expect("mux"), expected, "raw {raw}");
        }
    }

    #[tokio::test]
    async fn maps_proven_access_values() {
        let cases = [
            (0, GpuAccessPolicy::Unblocked),
            (1, GpuAccessPolicy::Blocked),
        ];
        for (raw, expected) in cases {
            let p = provider(vec![Ok(snapshot(Some(0), Some(raw)))]);
            assert_eq!(
                p.access_policy().await.expect("access"),
                expected,
                "raw {raw}"
            );
        }
    }

    #[tokio::test]
    async fn future_raw_is_unknown_without_guess() {
        // Два snapshot: mux_state() и access_policy() делают отдельные source reads.
        let p = provider(vec![
            Ok(snapshot(Some(7), Some(9))),
            Ok(snapshot(Some(7), Some(9))),
        ]);
        assert_eq!(p.mux_state().await.expect("mux"), GpuMuxState::Unknown);
        assert_eq!(
            p.access_policy().await.expect("access"),
            GpuAccessPolicy::Unknown
        );
    }

    #[tokio::test]
    async fn missing_file_is_unknown() {
        let p = provider(vec![Ok(snapshot(None, None)), Ok(snapshot(None, None))]);
        assert_eq!(p.mux_state().await.expect("mux"), GpuMuxState::Unknown);
        assert_eq!(
            p.access_policy().await.expect("access"),
            GpuAccessPolicy::Unknown
        );
    }

    #[tokio::test]
    async fn malformed_value_is_internal() {
        // Malformed содержимое моделируем через ScriptedSource? Нет: malformed
        // происходит в SysfsArmouryGpuSource::read_u32, поэтому проверим на
        // временном файле.
        let dir = tempfile::tempdir().expect("tempdir");
        let mux = dir.path().join("mux");
        std::fs::write(&mux, "not-a-number\n").expect("write");
        let dgpu = dir.path().join("dgpu");
        std::fs::write(&dgpu, "0\n").expect("write");
        let source = SysfsArmouryGpuSource::new(PathBuf::from(&mux), PathBuf::from(&dgpu));
        let p = ArmouryGpuProvider::new(source);
        let err = p.mux_state().await.expect_err("malformed mux");
        assert!(matches!(err, ProviderError::Internal(_)));
    }

    #[tokio::test]
    async fn source_error_propagates() {
        let p = provider(vec![Err(ProviderError::Dbus("backend down".into()))]);
        let err = p.mux_state().await.expect_err("source error");
        assert!(matches!(err, ProviderError::Dbus(_)));
    }

    #[tokio::test]
    async fn reads_are_fresh_not_cached() {
        let source = ScriptedSource::new(vec![
            Ok(snapshot(Some(0), Some(0))),
            Ok(snapshot(Some(1), Some(1))),
        ]);
        let p = ArmouryGpuProvider::new(source);
        assert_eq!(p.mux_state().await.expect("first"), GpuMuxState::Discrete);
        assert_eq!(
            p.mux_state().await.expect("second"),
            GpuMuxState::Integrated
        );
        assert_eq!(p.source.reads(), 2);
    }
}
