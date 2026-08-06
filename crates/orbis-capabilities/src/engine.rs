//! Движок capability-проб.
//!
//! На Этапе 2 строит матрицу из mock-частей; на Этапе 3+ части будут
//! собираться из реальных probe-источников (sysfs/D-Bus).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use orbis_core::capability::{Capability, CapabilityReason, CapabilityStatus, DeviceCapabilities, FeatureId, RiskLevel};
use orbis_core::identity::{BackendIdentity, DeviceIdentity};
use orbis_core::warning::{Warning, WarningSeverity};

/// Одна часть capability-данных (например, результат проверки одного backend).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityPart {
    /// Функция.
    pub feature: FeatureId,
    /// Статус.
    pub status: CapabilityStatus,
    /// Причина.
    pub reason: Option<CapabilityReason>,
}

/// Полный отчёт probe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeReport {
    /// Идентичность устройства (обезличенная).
    pub device: Option<DeviceIdentity>,
    /// Матрица возможностей.
    pub capabilities: DeviceCapabilities,
    /// Обнаруженные backend-и.
    pub backends: Vec<BackendIdentity>,
    /// Предупреждения probe.
    pub warnings: Vec<Warning>,
}

impl ProbeReport {
    /// Собрать отчёт из частей.
    pub fn from_parts(
        device: Option<DeviceIdentity>,
        parts: Vec<CapabilityPart>,
        backends: Vec<BackendIdentity>,
        warnings: Vec<Warning>,
    ) -> Self {
        let mut features = BTreeMap::new();
        for part in parts {
            features.insert(part.feature, Capability { status: part.status, reason: part.reason });
        }
        Self { device, capabilities: DeviceCapabilities { features }, backends, warnings }
    }

    /// Количество функций с заданным статусом.
    pub fn count_status(&self, status: CapabilityStatus) -> usize {
        self.capabilities.features.values().filter(|c| c.status == status).count()
    }
}

/// Удобный конструктор части без причины.
pub fn part(feature: FeatureId, status: CapabilityStatus) -> CapabilityPart {
    CapabilityPart { feature, status, reason: None }
}

/// Удобный конструктор части с причиной.
pub fn part_with_reason(
    feature: FeatureId,
    status: CapabilityStatus,
    reason: String,
    suggestion: String,
    endpoint: Option<String>,
    risk: RiskLevel,
) -> CapabilityPart {
    CapabilityPart {
        feature,
        status,
        reason: Some(CapabilityReason {
            reason,
            suggestion,
            backend: None,
            endpoint,
            requirement: None,
            risk,
            checked_at: None,
        }),
    }
}

/// Собрать отчёт из частей (обёртка для CLI-использования).
pub fn build_from_parts(
    device: Option<DeviceIdentity>,
    parts: Vec<CapabilityPart>,
    backends: Vec<BackendIdentity>,
) -> ProbeReport {
    ProbeReport::from_parts(device, parts, backends, Vec::new())
}

/// Проверить, что у каждой функции с не-`Supported` статусом есть причина.
pub fn ensure_reasons(report: &ProbeReport) -> Vec<Warning> {
    let mut warnings = Vec::new();
    for (feature, cap) in &report.capabilities.features {
        if cap.status != CapabilityStatus::Supported && cap.reason.is_none() {
            warnings.push(Warning {
                severity: WarningSeverity::Warning,
                code: "capability.missing_reason".into(),
                message: format!("{}: нет причины для статуса {}", feature.as_str(), cap.status.as_str()),
                details: None,
            });
        }
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbis_core::action::ActionRequirement;

    #[test]
    fn build_and_count() {
        let report = build_from_parts(
            None,
            vec![
                part(FeatureId::GpuMux, CapabilityStatus::SupportedWithRequirement),
                part(FeatureId::Anime, CapabilityStatus::Unsupported),
                part(FeatureId::Fans, CapabilityStatus::Supported),
            ],
            vec![BackendIdentity::simple("asusd")],
        );
        assert_eq!(report.count_status(CapabilityStatus::Supported), 1);
        assert_eq!(report.count_status(CapabilityStatus::Unsupported), 1);
        assert_eq!(report.capabilities.status(FeatureId::Anime), CapabilityStatus::Unsupported);
    }

    #[test]
    fn missing_reason_warns() {
        let report = build_from_parts(
            None,
            vec![
                part(FeatureId::GpuMux, CapabilityStatus::SupportedWithRequirement),
                part(FeatureId::Fans, CapabilityStatus::Supported),
            ],
            vec![],
        );
        let warnings = ensure_reasons(&report);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("gpu_mux"));
    }

    #[test]
    fn with_reason_no_warning() {
        let report = build_from_parts(
            None,
            vec![part_with_reason(
                FeatureId::GpuMux,
                CapabilityStatus::SupportedWithRequirement,
                "mux latched".into(),
                "reboot".into(),
                Some("/sys/...".into()),
                RiskLevel::Confirmation,
            )],
            vec![],
        );
        assert!(ensure_reasons(&report).is_empty());
        let reason = report.capabilities.reason(FeatureId::GpuMux).unwrap();
        assert_eq!(reason.suggestion, "reboot");
        assert_eq!(reason.requirement, None);
    }

    #[test]
    fn requirement_mapping() {
        // CapabilityReason не хранит requirement напрямую из part_with_reason;
        // проверяем что ActionRequirement сериализуется корректно в snapshot.
        let json = serde_json::to_string(&ActionRequirement::Reboot).unwrap();
        assert_eq!(json, "\"reboot\"");
    }

    #[test]
    fn report_serde_roundtrip() {
        let report = build_from_parts(
            None,
            vec![part(FeatureId::Fans, CapabilityStatus::Supported)],
            vec![BackendIdentity { id: "hwmon".into(), version: None, service: None }],
        );
        let json = serde_json::to_string(&report).unwrap();
        let back: ProbeReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back, report);
    }
}
