//! Хранилище конфигурации: TOML, атомарная запись, миграции, резервные копии.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use orbis_core::gpu::GpuMode;
use orbis_core::profile::PerformanceProfile;

use crate::CONFIG_VERSION;
use crate::paths;

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Ошибки конфигурации.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// Ошибка разрешения XDG-пути legacy config.
    #[error(transparent)]
    Path(#[from] paths::PathResolutionError),
    /// Ошибка ввода-вывода.
    #[error("io: {0}")]
    Io(#[from] io::Error),
    /// Некорректный TOML.
    #[error("toml parse: {0}")]
    Toml(#[from] toml::de::Error),
    /// Ошибка сериализации.
    #[error("toml serialize: {0}")]
    TomlSer(#[from] toml::ser::Error),
    /// Неподдерживаемая версия формата.
    #[error("неподдерживаемая config_version: {0} (ожидается {CONFIG_VERSION})")]
    UnsupportedVersion(u32),
    /// Ошибка миграции.
    #[error("миграция не выполнена: {0}")]
    Migration(String),
    /// Некорректная схема.
    #[error("схема: {0}")]
    Schema(String),
}

/// Секция UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    /// Тема ("dark" | "light").
    pub theme: String,
    /// Закрытие окна сворачивает в трей.
    pub close_to_tray: bool,
    /// Запуск свёрнутым.
    pub start_minimized: bool,
    /// Запоминать позицию окна.
    pub remember_position: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            close_to_tray: true,
            start_minimized: false,
            remember_position: true,
        }
    }
}

/// Правило автоматизации для источника питания.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PowerAutomationConfig {
    /// Профиль производительности.
    pub profile: Option<PerformanceProfile>,
    /// GPU-политика.
    pub gpu_policy: Option<GpuMode>,
    /// Политика частоты экрана.
    pub refresh_policy: Option<String>,
}

/// Секция автоматизации.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutomationConfig {
    /// Автоматизация включена.
    pub enabled: bool,
    /// Задержка реакции на событие питания, мс.
    pub power_event_delay_ms: u64,
    /// AC-правило.
    pub ac: PowerAutomationConfig,
    /// Правило батареи.
    pub battery: PowerAutomationConfig,
}

impl Default for AutomationConfig {
    fn default() -> Self {
        // Legacy/default configuration must be inert. Missing or malformed
        // compatibility config is never permission to synthesize hardware
        // intent or start reconciliation.
        Self {
            enabled: false,
            power_event_delay_ms: 1500,
            ac: PowerAutomationConfig::default(),
            battery: PowerAutomationConfig::default(),
        }
    }
}

/// Секция батареи.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BatteryConfig {
    /// Желаемый лимит зарядки (пользовательское намерение; применяется после
    /// подтверждения backend-ом). `None` означает, что Orbis не управляет
    /// threshold; конкретное значение обязано соответствовать Hardware1 ABI.
    pub charge_limit: Option<u8>,
}

impl Default for BatteryConfig {
    fn default() -> Self {
        // No implicit hardware intent in a missing legacy config.
        Self { charge_limit: None }
    }
}

/// Экспериментальные возможности.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExperimentalConfig {
    /// Экспериментальные функции включены в целом.
    pub enabled: bool,
    /// Undervolting (скрыт по умолчанию).
    pub undervolting: bool,
    /// Прямой raw WMI (запрещено в стабильной сборке).
    pub raw_wmi: bool,
}

/// Пользовательская конфигурация приложения.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Версия формата.
    pub config_version: u32,
    /// Секция UI.
    pub ui: UiConfig,
    /// Секция автоматизации.
    pub automation: AutomationConfig,
    /// Секция батареи.
    pub battery: BatteryConfig,
    /// Экспериментальные возможности.
    pub experimental: ExperimentalConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            config_version: CONFIG_VERSION,
            ui: UiConfig::default(),
            automation: AutomationConfig::default(),
            battery: BatteryConfig::default(),
            experimental: ExperimentalConfig::default(),
        }
    }
}

impl AppConfig {
    /// Валидация схемы (диапазоны и инварианты).
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.config_version != CONFIG_VERSION {
            return Err(ConfigError::UnsupportedVersion(self.config_version));
        }
        if !matches!(self.ui.theme.as_str(), "dark" | "light") {
            return Err(ConfigError::Schema(format!(
                "theme '{}' не в (dark, light)",
                self.ui.theme
            )));
        }
        if let Some(limit) = self.battery.charge_limit {
            if !(20..=100).contains(&limit) {
                return Err(ConfigError::Schema(format!(
                    "charge_limit {limit} вне [20,100]; используйте None, чтобы не управлять threshold"
                )));
            }
        }
        Ok(())
    }
}

/// Загрузить конфиг из каталога; при отсутствии — инертный дефолт.
pub fn load_from_dir(dir: &Path) -> Result<AppConfig, ConfigError> {
    let file = dir.join(paths::CONFIG_FILE);
    if !file.exists() {
        return Ok(AppConfig::default());
    }
    let text = fs::read_to_string(&file)?;
    from_toml_with_migration(&text)
}

/// Загрузить legacy-конфиг из fail-closed XDG-каталога.
///
/// Этот compatibility API не является источником автоматического desired state.
/// Missing/relative HOME/XDG is a typed error; this function never falls back
/// to the current working directory.
pub fn load_or_default() -> Result<AppConfig, ConfigError> {
    load_from_dir(&paths::config_dir_checked()?)
}

/// Разобрать TOML с миграцией версий.
pub fn from_toml_with_migration(text: &str) -> Result<AppConfig, ConfigError> {
    let mut cfg: AppConfig = toml::from_str(text)?;
    migrate(&mut cfg)?;
    cfg.validate()?;
    Ok(cfg)
}

/// Миграция формата (сейчас только v1; для будущих версий — цепочка миграций).
fn migrate(cfg: &mut AppConfig) -> Result<(), ConfigError> {
    match cfg.config_version {
        CONFIG_VERSION => Ok(()),
        v if v < CONFIG_VERSION => Err(ConfigError::Migration(format!(
            "конфиг v{v} устарел; миграции до v{CONFIG_VERSION} ещё не реализованы"
        ))),
        v => Err(ConfigError::UnsupportedVersion(v)),
    }
}

fn unique_temp_path(dir: &Path) -> PathBuf {
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    dir.join(format!(
        ".{}.tmp.{}.{}.{}",
        paths::CONFIG_FILE,
        std::process::id(),
        nanos,
        sequence
    ))
}

fn create_unique_temp(dir: &Path) -> Result<(File, PathBuf), ConfigError> {
    for _ in 0..64 {
        let path = unique_temp_path(dir);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        match options.open(&path) {
            Ok(file) => return Ok((file, path)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "unable to allocate a unique legacy config temporary file",
    )
    .into())
}

fn sync_parent_directory(dir: &Path) -> Result<(), ConfigError> {
    File::open(dir)?.sync_all()?;
    Ok(())
}

/// Durable atomic write: serialize first, create an exclusive same-directory
/// temporary file, preserve existing regular-file permissions, fsync temp,
/// rename, then fsync the parent directory.
pub fn save_to_dir(cfg: &AppConfig, dir: &Path) -> Result<PathBuf, ConfigError> {
    cfg.validate()?;
    let text = toml::to_string_pretty(cfg)?;
    fs::create_dir_all(dir)?;

    let final_path = dir.join(paths::CONFIG_FILE);
    let existing_permissions = match fs::symlink_metadata(&final_path) {
        Ok(metadata) if metadata.file_type().is_file() => Some(metadata.permissions()),
        Ok(_) => None,
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };

    let (mut temp, temp_path) = create_unique_temp(dir)?;
    if let Some(permissions) = existing_permissions {
        if let Err(error) = temp.set_permissions(permissions) {
            drop(temp);
            let _ = fs::remove_file(&temp_path);
            return Err(error.into());
        }
    }

    if let Err(error) = temp.write_all(text.as_bytes()) {
        drop(temp);
        let _ = fs::remove_file(&temp_path);
        return Err(error.into());
    }
    if let Err(error) = temp.flush() {
        drop(temp);
        let _ = fs::remove_file(&temp_path);
        return Err(error.into());
    }
    if let Err(error) = temp.sync_all() {
        drop(temp);
        let _ = fs::remove_file(&temp_path);
        return Err(error.into());
    }
    drop(temp);

    if let Err(error) = fs::rename(&temp_path, &final_path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error.into());
    }

    sync_parent_directory(dir)?;
    Ok(final_path)
}

/// Сделать резервную копию перед миграцией.
pub fn backup_before_migration(dir: &Path) -> Result<Option<PathBuf>, ConfigError> {
    let src = dir.join(paths::CONFIG_FILE);
    if !src.exists() {
        return Ok(None);
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let dst = dir.join(format!("{}.bak-{}", paths::CONFIG_FILE, stamp));
    fs::copy(&src, &dst)?;
    Ok(Some(dst))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_test_env() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    #[test]
    fn default_is_valid_and_hardware_inert() {
        let cfg = AppConfig::default();
        cfg.validate().unwrap();
        assert_eq!(cfg.config_version, CONFIG_VERSION);
        assert_eq!(cfg.ui.theme, "dark");
        assert!(!cfg.automation.enabled);
        assert_eq!(cfg.automation.ac, PowerAutomationConfig::default());
        assert_eq!(cfg.automation.battery, PowerAutomationConfig::default());
        assert_eq!(cfg.battery.charge_limit, None);
    }

    #[test]
    fn roundtrip_toml() {
        let cfg = AppConfig::default();
        let text = toml::to_string_pretty(&cfg).unwrap();
        let back = from_toml_with_migration(&text).unwrap();
        assert_eq!(back, cfg);
    }

    #[test]
    fn save_load_atomic_and_cleans_temp() {
        let td = temp_test_env();
        let dir = td.path().join("cfg");
        let cfg = AppConfig::default();
        save_to_dir(&cfg, &dir).unwrap();
        let loaded = load_from_dir(&dir).unwrap();
        assert_eq!(loaded, cfg);
        let leftovers = fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp."))
            .count();
        assert_eq!(leftovers, 0);
    }

    #[cfg(unix)]
    #[test]
    fn new_file_is_private_and_existing_permissions_are_preserved() {
        use std::os::unix::fs::PermissionsExt;

        let td = temp_test_env();
        let dir = td.path().join("cfg");
        let final_path = save_to_dir(&AppConfig::default(), &dir).unwrap();
        assert_eq!(fs::metadata(&final_path).unwrap().permissions().mode() & 0o777, 0o600);

        fs::set_permissions(&final_path, fs::Permissions::from_mode(0o640)).unwrap();
        save_to_dir(&AppConfig::default(), &dir).unwrap();
        assert_eq!(fs::metadata(&final_path).unwrap().permissions().mode() & 0o777, 0o640);
    }

    #[test]
    fn missing_file_returns_inert_default() {
        let td = temp_test_env();
        let cfg = load_from_dir(&td.path().join("nope")).unwrap();
        assert_eq!(cfg, AppConfig::default());
        assert!(!cfg.automation.enabled);
        assert_eq!(cfg.battery.charge_limit, None);
    }

    #[test]
    fn bad_theme_rejected() {
        let cfg = AppConfig {
            ui: UiConfig {
                theme: "neon".into(),
                ..UiConfig::default()
            },
            ..AppConfig::default()
        };
        assert!(matches!(cfg.validate(), Err(ConfigError::Schema(_))));
    }

    #[test]
    fn wrong_version_rejected() {
        let cfg = AppConfig {
            config_version: 99,
            ..AppConfig::default()
        };
        assert!(matches!(
            cfg.validate(),
            Err(ConfigError::UnsupportedVersion(99))
        ));
    }

    #[test]
    fn charge_limit_range() {
        let below_minimum = AppConfig {
            battery: BatteryConfig {
                charge_limit: Some(19),
            },
            ..AppConfig::default()
        };
        assert!(below_minimum.validate().is_err());

        let minimum = AppConfig {
            battery: BatteryConfig {
                charge_limit: Some(20),
            },
            ..AppConfig::default()
        };
        assert!(minimum.validate().is_ok());

        let maximum = AppConfig {
            battery: BatteryConfig {
                charge_limit: Some(100),
            },
            ..AppConfig::default()
        };
        assert!(maximum.validate().is_ok());

        let above_maximum = AppConfig {
            battery: BatteryConfig {
                charge_limit: Some(101),
            },
            ..AppConfig::default()
        };
        assert!(above_maximum.validate().is_err());

        let unmanaged = AppConfig {
            battery: BatteryConfig { charge_limit: None },
            ..AppConfig::default()
        };
        assert!(unmanaged.validate().is_ok());
    }

    #[test]
    fn backup_created() {
        let td = temp_test_env();
        let dir = td.path().join("cfg");
        save_to_dir(&AppConfig::default(), &dir).unwrap();
        let backup = backup_before_migration(&dir).unwrap();
        assert!(backup.is_some());
        assert!(backup.unwrap().exists());
    }

    #[test]
    fn checked_path_resolver_never_selects_cwd_without_home() {
        assert_eq!(
            paths::config_dir_with_checked(None, None),
            Err(paths::PathResolutionError::MissingHome)
        );
    }
}
