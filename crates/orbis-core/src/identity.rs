//! Идентичность устройства и backend-ов.

use serde::{Deserialize, Serialize};

/// Идентичность устройства (DMI). Не содержит персональных данных
/// (серийный номер и т.п. намеренно отсутствуют).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceIdentity {
    /// Вендор (например, "ASUSTeK COMPUTER INC.").
    pub vendor: String,
    /// Модель.
    pub product: String,
    /// Board name.
    pub board: String,
    /// Версия BIOS.
    pub bios_version: String,
    /// Дата BIOS.
    pub bios_date: String,
}

impl DeviceIdentity {
    /// Компактная строка "vendor product (board)".
    pub fn display_name(&self) -> String {
        format!("{} {} ({})", self.vendor, self.product, self.board)
    }
}

/// Идентичность backend (провайдера).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendIdentity {
    /// Идентификатор backend (например, "asusd").
    pub id: String,
    /// Версия (если известна).
    pub version: Option<String>,
    /// D-Bus service name (если применимо).
    pub service: Option<String>,
}

impl BackendIdentity {
    /// Простой backend без версии.
    pub fn simple(id: &str) -> Self {
        Self { id: id.to_string(), version: None, service: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_name() {
        let d = DeviceIdentity {
            vendor: "ASUSTeK COMPUTER INC.".into(),
            product: "ASUS TUF Gaming A17".into(),
            board: "FA707NV".into(),
            bios_version: "FA707NV.316".into(),
            bios_date: "11/04/2024".into(),
        };
        assert!(d.display_name().contains("FA707NV"));
    }

    #[test]
    fn backend_simple() {
        let b = BackendIdentity::simple("asusd");
        assert_eq!(b.id, "asusd");
        assert!(b.version.is_none());
    }
}
