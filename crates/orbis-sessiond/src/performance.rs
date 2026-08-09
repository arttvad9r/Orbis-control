//! Read-only kernel `platform_profile` adapter для Performance Mode.
//!
//! Реализует `PerformanceProvider` через read-only sysfs
//! `/sys/firmware/acpi/platform_profile` и `/sys/firmware/acpi/platform_profile_choices`.
//!
//! - путь к sysfs передаётся в source извне; source сам не открывает файлы
//!   с записью, не выполняет hardware mutation и не создаёт runtime;
//! - каждый вызов `current_profile()`/`profiles()` выполняет новый
//!   authoritative read (кэш отсутствует);
//! - mapping символьных значений ядра в `PerformanceProfile` использует
//!   существующий domain parse (`orbis_core::profile::PerformanceProfile::parse`):
//!   `quiet → Silent`, `balanced → Balanced`, `performance → Turbo`,
//!   `low-power → Silent`; неизвестное значение не clamp-ится и не
//!   подбирается — возвращается `ProviderError::Unsupported`;
//! - mutation-методы `PerformanceProvider` возвращают
//!   `ProviderError::Unsupported` и не выполняют I/O.

use std::time::Duration;

use async_trait::async_trait;
use orbis_core::action::ApplyResult;
use orbis_core::diagnostics::DiagnosticEntry;
use orbis_core::identity::BackendIdentity;
use orbis_core::profile::PerformanceProfile;
use orbis_providers::error::{ProviderError, ValidationResult};
use orbis_providers::traits::{PerformanceProvider, Provider, ProviderHealth};

/// Raw snapshot kernel platform_profile (независим от domain).
///
/// Domain validation здесь не выполняется.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelPlatformProfileSnapshot {
    /// Текущее символьное значение (например, "quiet", "balanced", "performance").
    pub current: String,
    /// Доступные символьные значения в порядке, отданном ядром.
    pub choices: Vec<String>,
}

/// Testable источник kernel platform_profile snapshot.
#[async_trait]
pub trait KernelPlatformProfileSource: Send + Sync {
    /// Прочитать snapshot (authoritative, без кэша).
    async fn read_snapshot(&self) -> Result<KernelPlatformProfileSnapshot, ProviderError>;
}

/// Реальный sysfs источник.
///
/// Хранит пути к `platform_profile` и `platform_profile_choices`; I/O
/// начинается только в `read_snapshot().await`. Конструктор не выполняет I/O,
/// не проверяет существование файлов и не открывает их на запись.
pub struct SysfsKernelPlatformProfileSource {
    current_path: std::path::PathBuf,
    choices_path: std::path::PathBuf,
}

impl SysfsKernelPlatformProfileSource {
    /// Создать источник с явными путями (по умолчанию — стандартные ABI paths).
    ///
    /// Пути можно передать снаружи, что делает source тестируемым на
    /// временных файлах; по умолчанию используются
    /// `/sys/firmware/acpi/platform_profile` и
    /// `/sys/firmware/acpi/platform_profile_choices`.
    pub fn new(current_path: std::path::PathBuf, choices_path: std::path::PathBuf) -> Self {
        Self {
            current_path,
            choices_path,
        }
    }
}

impl Default for SysfsKernelPlatformProfileSource {
    fn default() -> Self {
        Self::new(
            std::path::PathBuf::from("/sys/firmware/acpi/platform_profile"),
            std::path::PathBuf::from("/sys/firmware/acpi/platform_profile_choices"),
        )
    }
}

impl SysfsKernelPlatformProfileSource {
    /// Прочитать один файл, trim содержимое, вернуть строку.
    fn read_trimmed(path: &std::path::Path) -> Result<String, ProviderError> {
        let content = std::fs::read_to_string(path).map_err(ProviderError::Io)?;
        let trimmed = content.trim().to_string();
        if trimmed.is_empty() {
            return Err(ProviderError::Internal(format!(
                "kernel platform_profile: файл '{}' пуст",
                path.display()
            )));
        }
        Ok(trimmed)
    }
}

#[async_trait]
impl KernelPlatformProfileSource for SysfsKernelPlatformProfileSource {
    async fn read_snapshot(&self) -> Result<KernelPlatformProfileSnapshot, ProviderError> {
        // Простое read-only чтение маленьких файлов выполняется синхронно в
        // async context: операции tiny и не блокируют runtime заметно.
        let current = Self::read_trimmed(&self.current_path)?;
        let choices_raw = Self::read_trimmed(&self.choices_path)?;
        let choices = choices_raw
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();
        Ok(KernelPlatformProfileSnapshot { current, choices })
    }
}

/// Read-only Performance provider над kernel platform_profile source.
///
/// `S` — source (реальный sysfs или scripted в тестах); источник передаётся
/// в конструкторе, который не выполняет I/O.
pub struct KernelPerformanceProvider<S> {
    source: S,
}

impl<S> KernelPerformanceProvider<S> {
    /// Создать provider над source.
    ///
    /// Не открывает sysfs и не выполняет I/O.
    pub fn new(source: S) -> Self {
        Self { source }
    }
}

impl<S> Provider for KernelPerformanceProvider<S>
where
    S: KernelPlatformProfileSource,
{
    fn id(&self) -> &'static str {
        "kernel-platform-profile"
    }

    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("kernel-platform-profile")
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }

    fn explain_unsupported(&self, feature: &str) -> String {
        format!("kernel platform_profile read-only backend: функция '{feature}' недоступна")
    }

    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }

    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        vec![DiagnosticEntry::new(
            "provider.kernel-platform-profile",
            "read-only kernel platform_profile backend (без hardware writes)",
        )]
    }
}

/// Преобразовать символьное значение ядра в `PerformanceProfile`.
///
/// Использует существующий domain parse (`PerformanceProfile::parse`), который
/// закрепляет: `quiet/low-power → Silent`, `balanced → Balanced`,
/// `performance → Turbo`. Неизвестное значение → `ProviderError::Unsupported`
/// (без clamp/подбора ближайшего).
fn profile_from_kernel(symbol: &str) -> Result<PerformanceProfile, ProviderError> {
    PerformanceProfile::parse(symbol).map_err(|_| {
        ProviderError::Unsupported(format!(
            "kernel platform_profile: неизвестный профиль '{symbol}'"
        ))
    })
}

#[async_trait]
impl<S> PerformanceProvider for KernelPerformanceProvider<S>
where
    S: KernelPlatformProfileSource,
{
    async fn profiles(&self) -> Result<Vec<PerformanceProfile>, ProviderError> {
        let snapshot = self.source.read_snapshot().await?;
        // Не скрываем неизвестные значения: каждый choice либо маппится, либо
        // провайдер возвращает Unsupported с пояснением.
        snapshot
            .choices
            .iter()
            .map(|s| profile_from_kernel(s))
            .collect()
    }

    async fn current_profile(&self) -> Result<PerformanceProfile, ProviderError> {
        let snapshot = self.source.read_snapshot().await?;
        profile_from_kernel(&snapshot.current)
    }

    async fn set_profile(
        &self,
        _profile: PerformanceProfile,
    ) -> Result<ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported(
            "kernel platform_profile read-only backend: set_profile недоступна".into(),
        ))
    }

    async fn profile_on_ac(&self) -> Result<Option<PerformanceProfile>, ProviderError> {
        // kernel platform_profile не разделяет AC/battery профили.
        Ok(None)
    }

    async fn profile_on_battery(&self) -> Result<Option<PerformanceProfile>, ProviderError> {
        // kernel platform_profile не разделяет AC/battery профили.
        Ok(None)
    }

    fn validate_set_profile(&self, _profile: PerformanceProfile) -> ValidationResult {
        ValidationResult::invalid("read-only backend: запись profile не поддерживается")
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// Тестовый источник: очередь заранее заданных snapshots, счётчик вызовов.
    struct ScriptedSource {
        snapshots: std::sync::Mutex<
            std::collections::VecDeque<Result<KernelPlatformProfileSnapshot, ProviderError>>,
        >,
        reads: AtomicUsize,
    }

    impl ScriptedSource {
        fn new(snapshots: Vec<Result<KernelPlatformProfileSnapshot, ProviderError>>) -> Self {
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
    impl KernelPlatformProfileSource for ScriptedSource {
        async fn read_snapshot(&self) -> Result<KernelPlatformProfileSnapshot, ProviderError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            self.snapshots.lock().unwrap().pop_front().ok_or_else(|| {
                ProviderError::Internal("scripted source: очередь исчерпана".into())
            })?
        }
    }

    fn snapshot(current: &str, choices: &[&str]) -> KernelPlatformProfileSnapshot {
        KernelPlatformProfileSnapshot {
            current: current.to_string(),
            choices: choices.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[tokio::test]
    async fn maps_known_current_values() {
        let cases = [
            ("quiet", PerformanceProfile::Silent),
            ("balanced", PerformanceProfile::Balanced),
            ("performance", PerformanceProfile::Turbo),
            ("low-power", PerformanceProfile::Silent),
        ];
        for (symbol, expected) in cases {
            let p = KernelPerformanceProvider::new(ScriptedSource::new(vec![Ok(snapshot(
                symbol,
                &["quiet", "balanced", "performance"],
            ))]));
            assert_eq!(
                p.current_profile().await.expect("current"),
                expected,
                "symbol '{symbol}'"
            );
        }
    }

    #[tokio::test]
    async fn maps_known_choices() {
        let p = KernelPerformanceProvider::new(ScriptedSource::new(vec![Ok(snapshot(
            "quiet",
            &["quiet", "balanced", "performance"],
        ))]));
        let profiles = p.profiles().await.expect("profiles");
        assert_eq!(
            profiles,
            vec![
                PerformanceProfile::Silent,
                PerformanceProfile::Balanced,
                PerformanceProfile::Turbo,
            ]
        );
    }

    #[tokio::test]
    async fn unknown_current_is_unsupported_without_guess() {
        let p = KernelPerformanceProvider::new(ScriptedSource::new(vec![Ok(snapshot(
            "gaming",
            &["quiet", "balanced", "performance"],
        ))]));
        let err = p.current_profile().await.expect_err("unknown current");
        assert!(matches!(err, ProviderError::Unsupported(_)));
        assert!(err.to_string().contains("gaming"));
    }

    #[tokio::test]
    async fn unknown_choice_is_unsupported() {
        let p = KernelPerformanceProvider::new(ScriptedSource::new(vec![Ok(snapshot(
            "quiet",
            &["quiet", "gaming", "performance"],
        ))]));
        let err = p.profiles().await.expect_err("unknown choice");
        assert!(matches!(err, ProviderError::Unsupported(_)));
        assert!(err.to_string().contains("gaming"));
    }

    #[tokio::test]
    async fn source_error_propagates() {
        let p = KernelPerformanceProvider::new(ScriptedSource::new(vec![Err(
            ProviderError::Dbus("backend down".into()),
        )]));
        let err = p.current_profile().await.expect_err("source error");
        assert!(matches!(err, ProviderError::Dbus(_)));
    }

    #[tokio::test]
    async fn reads_are_fresh_not_cached() {
        let source = ScriptedSource::new(vec![
            Ok(snapshot("quiet", &["quiet", "balanced", "performance"])),
            Ok(snapshot(
                "performance",
                &["quiet", "balanced", "performance"],
            )),
        ]);
        let p = KernelPerformanceProvider::new(source);

        assert_eq!(
            p.current_profile().await.expect("first"),
            PerformanceProfile::Silent
        );
        assert_eq!(
            p.current_profile().await.expect("second"),
            PerformanceProfile::Turbo
        );
        // Каждый вызов делает новый authoritative read; кэш отсутствует.
        assert_eq!(p.source.reads(), 2);
    }

    #[tokio::test]
    async fn mutations_are_unsupported_without_source_access() {
        let source = ScriptedSource::new(vec![Ok(snapshot(
            "quiet",
            &["quiet", "balanced", "performance"],
        ))]);
        let p = KernelPerformanceProvider::new(source);

        assert!(matches!(
            p.set_profile(PerformanceProfile::Turbo)
                .await
                .expect_err("set unsupported"),
            ProviderError::Unsupported(_)
        ));
        assert!(matches!(
            p.validate_set_profile(PerformanceProfile::Turbo),
            ValidationResult::Invalid(_)
        ));
        assert_eq!(p.source.reads(), 0);
    }

    #[tokio::test]
    async fn ac_battery_profiles_are_none() {
        let p = KernelPerformanceProvider::new(ScriptedSource::new(vec![Ok(snapshot(
            "quiet",
            &["quiet", "balanced", "performance"],
        ))]));
        assert_eq!(p.profile_on_ac().await.expect("ac"), None);
        assert_eq!(p.profile_on_battery().await.expect("battery"), None);
    }
}
