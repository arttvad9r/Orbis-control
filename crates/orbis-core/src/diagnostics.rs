//! Диагностика.

use serde::{Deserialize, Serialize};

use crate::warning::WarningSeverity;

/// Запись диагностики.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticEntry {
    /// Ключ (стабильный).
    pub key: String,
    /// Значение.
    pub value: String,
    /// Серьёзность.
    pub severity: WarningSeverity,
    /// Источник (провайдер/backend).
    pub source: Option<String>,
}

impl DiagnosticEntry {
    /// Обычная запись.
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self { key: key.into(), value: value.into(), severity: WarningSeverity::Info, source: None }
    }

    /// Запись с серьёзностью.
    pub fn with_severity(
        key: impl Into<String>,
        value: impl Into<String>,
        severity: WarningSeverity,
    ) -> Self {
        Self { key: key.into(), value: value.into(), severity, source: None }
    }
}

/// Полный диагностический отчёт.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticReport {
    /// Записи.
    pub entries: Vec<DiagnosticEntry>,
    /// Отметка времени генерации (Unix seconds).
    pub generated_at: u64,
}

impl DiagnosticReport {
    /// Найти запись по ключу.
    pub fn get(&self, key: &str) -> Option<&DiagnosticEntry> {
        self.entries.iter().find(|e| e.key == key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_lookup() {
        let mut r = DiagnosticReport::default();
        r.entries.push(DiagnosticEntry::new("kernel", "7.1.6"));
        assert_eq!(r.get("kernel").unwrap().value, "7.1.6");
        assert!(r.get("missing").is_none());
    }
}
