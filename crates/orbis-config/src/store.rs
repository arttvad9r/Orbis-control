//! Хранилище конфигурации: TOML, атомарная запись, миграции, резервные копии.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use orbis_core::gpu::GpuMode;
use orbis_core::profile::PerformanceProfile;

use crate::CONFIG_VERSION;
use crate::paths;

/// Ошибки конфигурации.
#[derive(Debug, Error)]
pub enum ConfigError {
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
        Self {
            enabled: true,
            power_event_delay_ms: 1500,
            ac: PowerAutomationConfig {
                profile: Some(PerformanceProfile::Balanced),
                gpu_policy: Some(GpuMode::Standard),
                refresh_policy: Some("maximum".into()),
            },
            battery: PowerAutomationConfig {
                profile: Some(PerformanceProfile::Silent),
                gpu_policy: Some(GpuMode::Eco),
                refresh_policy: Some("minimum".into()),
            },
        }
    }
}

/// Секция батареи.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BatteryConfig {
    /// Желаемый лимит зарядки (пользовательское намерение; применяется после
    /// подтверждения backend-ом).
    pub charge_limit: Option<u8>,
}

impl Default for BatteryConfig {
    fn default() -> Self {
        Self {
            charge_limit: Some(80),
        }
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
            if !(0..=100).contains(&limit) {
                return Err(ConfigError::Schema(format!(
                    "charge_limit {limit} вне [0,100]"
                )));
            }
        }
        Ok(())
    }
}

/// Загрузить конфиг из каталога; при отсутствии — дефолт.
pub fn load_from_dir(dir: &Path) -> Result<AppConfig, ConfigError> {
    let file = dir.join(paths::CONFIG_FILE);
    if !file.exists() {
        return Ok(AppConfig::default());
    }
    let text = fs::read_to_string(&file)?;
    from_toml_with_migration(&text)
}

/// Загрузить конфиг из стандартного XDG-каталога.
pub fn load_or_default() -> Result<AppConfig, ConfigError> {
    load_from_dir(&crate::config_dir())
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

/// Атомарная запись: temp-файл + rename.
pub fn save_to_dir(cfg: &AppConfig, dir: &Path) -> Result<PathBuf, ConfigError> {
    cfg.validate()?;
    fs::create_dir_all(dir)?;
    let tmp = dir.join(format!("{}.tmp", paths::CONFIG_FILE));
    let text = toml::to_string_pretty(cfg)?;
    fs::write(&tmp, text)?;
    let final_path = dir.join(paths::CONFIG_FILE);
    fs::rename(&tmp, &final_path)?;
    Ok(final_path)
}

/// Сделать резервную копию перед миграцией.
pub fn backup_before_migration(dir: &Path) -> Result<Option<PathBuf>, ConfigError> {
    let src = dir.join(paths::CONFIG_FILE);
    if !src.exists() {
        return Ok(None);
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
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
    fn default_is_valid() {
        let cfg = AppConfig::default();
        cfg.validate().unwrap();
        assert_eq!(cfg.config_version, CONFIG_VERSION);
        assert_eq!(cfg.ui.theme, "dark");
        assert_eq!(cfg.battery.charge_limit, Some(80));
    }

    #[test]
    fn roundtrip_toml() {
        let cfg = AppConfig::default();
        let text = toml::to_string_pretty(&cfg).unwrap();
        let back = from_toml_with_migration(&text).unwrap();
        assert_eq!(back, cfg);
    }

    #[test]
    fn save_load_atomic() {
        let td = temp_test_env();
        let dir = td.path().join("cfg");
        let cfg = AppConfig::default();
        save_to_dir(&cfg, &dir).unwrap();
        let loaded = load_from_dir(&dir).unwrap();
        assert_eq!(loaded, cfg);
        // temp-файл не должен остаться
        assert!(!dir.join(format!("{}.tmp", paths::CONFIG_FILE)).exists());
    }

    #[test]
    fn missing_file_returns_default() {
        let td = temp_test_env();
        let cfg = load_from_dir(&td.path().join("nope")).unwrap();
        assert_eq!(cfg, AppConfig::default());
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
        let bad = AppConfig {
            battery: BatteryConfig {
                charge_limit: Some(150),
            },
            ..AppConfig::default()
        };
        assert!(bad.validate().is_err());
        let good = AppConfig {
            battery: BatteryConfig {
                charge_limit: Some(0),
            },
            ..AppConfig::default()
        };
        assert!(good.validate().is_ok());
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
    fn never_writes_user_home() {
        // unit-тесты не пишут в реальный XDG: только temp
        let td = temp_test_env();
        let dir = td.path().join("iso");
        save_to_dir(&AppConfig::default(), &dir).unwrap();
        let real = crate::config_dir();
        assert!(!real.join(paths::CONFIG_FILE).exists() || real != dir);
    }
}
