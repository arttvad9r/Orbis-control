//! Работа с фикстурами: загрузка expected-capabilities, проверка приватности.

use std::fs;
use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

use orbis_core::capability::{Capability, CapabilityReason, CapabilityStatus, DeviceCapabilities, FeatureId, RiskLevel};
use orbis_core::identity::BackendIdentity;

/// Ошибки работы с фикстурами.
#[derive(Debug, Error)]
pub enum FixtureError {
    /// Ошибка ввода-вывода.
    #[error("fixture io: {0}")]
    Io(#[source] std::io::Error),
    /// Ошибка JSON.
    #[error("fixture json: {0}")]
    Json(#[from] serde_json::Error),
    /// Неизвестная функция.
    #[error("неизвестная функция в фикстуре: {0}")]
    UnknownFeature(String),
}

/// Сырая фикстура expected-capabilities.json (обезличенная).
#[derive(Debug, Clone, Deserialize)]
pub struct ExpectedCapabilitiesFixture {
    /// Версия схемы.
    #[serde(default)]
    pub schema_version: u32,
    /// Описание устройства.
    #[serde(default)]
    pub device: String,
    /// Источник probe.
    #[serde(default)]
    pub source_probe: String,
    /// Ядро.
    #[serde(default)]
    pub kernel: String,
    /// Матрица функций (raw).
    pub features: std::collections::BTreeMap<String, ExpectedFeature>,
    /// Backend-и.
    #[serde(default)]
    pub backends: std::collections::BTreeMap<String, ExpectedBackend>,
}

/// Сырая запись функции в фикстуре.
#[derive(Debug, Clone, Deserialize)]
pub struct ExpectedFeature {
    /// Статус.
    pub status: String,
    /// Причина.
    #[serde(default)]
    pub reason: Option<String>,
    /// Текущее значение.
    #[serde(default)]
    pub current: Option<serde_json::Value>,
    /// Backend.
    #[serde(default)]
    pub backend: Option<String>,
    /// Endpoint.
    #[serde(default)]
    pub endpoint: Option<String>,
    /// Requirement.
    #[serde(default)]
    pub requirement: Option<String>,
    /// Risk.
    #[serde(default)]
    pub risk: Option<String>,
}

/// Сырая запись backend в фикстуре.
#[derive(Debug, Clone, Deserialize)]
pub struct ExpectedBackend {
    /// Присутствует ли.
    #[serde(default)]
    pub present: bool,
    /// Версия.
    #[serde(default)]
    pub version: Option<String>,
    /// Имя сервиса.
    #[serde(default)]
    pub service: Option<String>,
}

/// Разобрать статус из строки (регистронезависимо; принимает и snake_case,
/// и camelCase/слейшн-формы из фикстур).
pub fn parse_status(s: &str) -> Option<CapabilityStatus> {
    match s.to_ascii_lowercase().replace('-', "_").as_str() {
        "supported" => Some(CapabilityStatus::Supported),
        "supported_with_requirement" | "supportedwithrequirement" => {
            Some(CapabilityStatus::SupportedWithRequirement)
        }
        "read_only" | "readonly" => Some(CapabilityStatus::ReadOnly),
        "temporarily_unavailable" | "temporarilyunavailable" => {
            Some(CapabilityStatus::TemporarilyUnavailable)
        }
        "unsupported" => Some(CapabilityStatus::Unsupported),
        "backend_missing" | "backendmissing" => Some(CapabilityStatus::BackendMissing),
        "permission_denied" | "permissiondenied" => Some(CapabilityStatus::PermissionDenied),
        "experimental" => Some(CapabilityStatus::Experimental),
        "conflicted" => Some(CapabilityStatus::Conflicted),
        "unknown" => Some(CapabilityStatus::Unknown),
        _ => None,
    }
}

/// Разобрать риск (регистронезависимо).
fn parse_risk(s: Option<&str>) -> RiskLevel {
    match s.map(|s| s.to_ascii_lowercase()) {
        Some(v) if v == "confirmation" => RiskLevel::Confirmation,
        Some(v) if v == "dangerous" => RiskLevel::Dangerous,
        Some(v) if v == "experimental" => RiskLevel::Experimental,
        _ => RiskLevel::Safe,
    }
}

impl ExpectedCapabilitiesFixture {
    /// Загрузить фикстуру из файла.
    pub fn load(path: &Path) -> Result<Self, FixtureError> {
        let text = fs::read_to_string(path).map_err(FixtureError::Io)?;
        Ok(serde_json::from_str(&text)?)
    }

    /// Преобразовать в типизированную матрицу возможностей.
    pub fn to_device_capabilities(&self) -> Result<DeviceCapabilities, FixtureError> {
        let mut features = std::collections::BTreeMap::new();
        for (name, raw) in &self.features {
            let feature = feature_from_str(name).ok_or_else(|| FixtureError::UnknownFeature(name.clone()))?;
            let status = parse_status(&raw.status).ok_or_else(|| FixtureError::UnknownFeature(format!("{name}:{}", raw.status)))?;
            let reason = raw.reason.clone().map(|reason| CapabilityReason {
                reason,
                suggestion: String::new(),
                backend: raw.backend.clone().map(|b| BackendIdentity::simple(&b)),
                endpoint: raw.endpoint.clone(),
                requirement: None,
                risk: parse_risk(raw.risk.as_deref()),
                checked_at: None,
            });
            features.insert(feature, Capability { status, reason });
        }
        Ok(DeviceCapabilities { features })
    }

    /// Список backend-ов.
    pub fn backend_list(&self) -> Vec<BackendIdentity> {
        self.backends
            .iter()
            .filter(|(_, b)| b.present)
            .map(|(id, b)| BackendIdentity { id: id.clone(), version: b.version.clone(), service: b.service.clone() })
            .collect()
    }
}

/// Функция по строковому имени (должна совпадать с FeatureId::as_str).
fn feature_from_str(s: &str) -> Option<FeatureId> {
    FeatureId::ALL.iter().copied().find(|f| f.as_str() == s)
}

/// Проверка приватности фикстуры: каталог не должен содержать персональные данные.
///
/// Возвращает список найденных проблем (пусто = чисто).
pub fn privacy_check_fixture_dir(dir: &Path) -> Vec<String> {
    const FORBIDDEN: &[&str] = &[
        "serial_number", "serial", "hostname", "machine-id", "machine_id",
        "mac=", "uuid", "/home/", "password", "token", "secret",
    ];
    let mut problems = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return vec!["fixture dir not readable".into()],
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let content = fs::read_to_string(&path).unwrap_or_default().to_lowercase();
        for pat in FORBIDDEN {
            if content.contains(pat) {
                problems.push(format!("{}: запрещённый паттерн '{}'", path.display(), pat));
            }
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_status_all() {
        assert_eq!(parse_status("supported"), Some(CapabilityStatus::Supported));
        assert_eq!(parse_status("read_only"), Some(CapabilityStatus::ReadOnly));
        assert_eq!(parse_status("permission_denied"), Some(CapabilityStatus::PermissionDenied));
        assert_eq!(parse_status("bogus"), None);
    }

    #[test]
    fn privacy_check_finds_and_passes() {
        let td = tempfile::tempdir().unwrap();
        // чистый файл
        fs::write(td.path().join("ok.json"), r#"{"a": 1}"#).unwrap();
        assert!(privacy_check_fixture_dir(td.path()).is_empty());

        // грязный файл
        fs::write(td.path().join("bad.toml"), "serial_number = 42").unwrap();
        let problems = privacy_check_fixture_dir(td.path());
        assert!(!problems.is_empty());
        assert!(problems[0].contains("serial"));
    }

    #[test]
    fn load_real_fixture() {
        // Реальная обезличенная фикстура с нашего ноутбука.
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/hardware/fa707nv/expected-capabilities.json");
        let fixture = ExpectedCapabilitiesFixture::load(&path).expect("fixture");
        let caps = fixture.to_device_capabilities().expect("caps");
        assert_eq!(caps.status(FeatureId::GpuMux), CapabilityStatus::SupportedWithRequirement);
        assert_eq!(caps.status(FeatureId::Anime), CapabilityStatus::Unsupported);
        assert_eq!(caps.status(FeatureId::CpuBoost), CapabilityStatus::PermissionDenied);
        assert_eq!(caps.status(FeatureId::PptPl1Spl), CapabilityStatus::ReadOnly);
        // фикстура не должна содержать персональных данных
        let problems = privacy_check_fixture_dir(path.parent().unwrap());
        assert!(problems.is_empty(), "проблемы: {problems:?}");
    }
}
