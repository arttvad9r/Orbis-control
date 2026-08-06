//! Статус провайдера.

use serde::{Deserialize, Serialize};

/// Здоровье провайдера/backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderStatus {
    /// Работает нормально.
    Healthy,
    /// Работает с ограничениями.
    Degraded,
    /// Недоступен.
    Unavailable,
}

impl ProviderStatus {
    /// Имя для диагностики.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Degraded => "degraded",
            Self::Unavailable => "unavailable",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(ProviderStatus::Healthy.as_str(), "healthy");
        assert_eq!(ProviderStatus::Unavailable.as_str(), "unavailable");
    }
}
