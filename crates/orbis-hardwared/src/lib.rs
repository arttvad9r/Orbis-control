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

use orbis_core::action::ApplyResult;
use orbis_core::profile::PerformanceProfile;
use orbis_providers::error::ProviderError;

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
    /// Создать writer над реальным `std::fs` с явными путями (tests: temp).
    pub fn new(profile_path: PathBuf, choices_path: PathBuf) -> Self {
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
    /// Создать writer над инъектируемым IO (тесты: fake backend).
    pub fn with_io(io: S, profile_path: PathBuf, choices_path: PathBuf) -> Self {
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
}
