//! Daemon-side product policy для sessiond.

use crate::upower::ChargeLimitBounds;

/// Политика диапазона charge-limit (daemon-side product policy).
///
/// Это НЕ UPower-reported hardware capability: UPower не предоставляет
/// универсальные min/max/step. Значения согласованы с текущим UI-диапазоном
/// и централизованы здесь как единственный production владелец default
/// bounds; позднее их можно заменить конфигурацией или backend capability
/// без изменения `ChargeLimitBounds` API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChargeLimitPolicy {
    /// Нижняя граница диапазона, %.
    pub min_percent: u8,
    /// Верхняя граница диапазона, %.
    pub max_percent: u8,
    /// Шаг диапазона, %.
    pub step_percent: u8,
}

/// Конфигурация sessiond (daemon-side product policy).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessiondConfig {
    /// Политика charge-limit диапазона.
    pub charge_limit: ChargeLimitPolicy,
}

impl Default for SessiondConfig {
    fn default() -> Self {
        Self {
            charge_limit: ChargeLimitPolicy {
                min_percent: 40,
                max_percent: 100,
                step_percent: 5,
            },
        }
    }
}

impl SessiondConfig {
    /// Вернуть `ChargeLimitBounds` из текущей product policy.
    pub fn charge_limit_bounds(&self) -> ChargeLimitBounds {
        ChargeLimitBounds {
            min_percent: self.charge_limit.min_percent,
            max_percent: self.charge_limit.max_percent,
            step_percent: self.charge_limit.step_percent,
        }
    }
}

#[cfg(test)]
mod tests {
    use orbis_core::battery::ChargeLimit;
    use orbis_core::newtypes::Percent;

    use super::*;

    #[test]
    fn default_policy_is_40_100_5() {
        let config = SessiondConfig::default();
        assert_eq!(config.charge_limit.min_percent, 40);
        assert_eq!(config.charge_limit.max_percent, 100);
        assert_eq!(config.charge_limit.step_percent, 5);
    }

    #[test]
    fn default_policy_produces_expected_bounds() {
        let bounds = SessiondConfig::default().charge_limit_bounds();
        assert_eq!(bounds.min_percent, 40);
        assert_eq!(bounds.max_percent, 100);
        assert_eq!(bounds.step_percent, 5);
    }

    #[test]
    fn default_policy_is_domain_valid() {
        let bounds = SessiondConfig::default().charge_limit_bounds();
        // Существующий domain constructor подтверждает согласованность policy.
        let limit = ChargeLimit::new(
            true,
            Some(Percent::new(80).expect("range")),
            Some(
                orbis_core::battery::ChargeLimitBounds::new(
                    Percent::new(bounds.min_percent).expect("range"),
                    Percent::new(bounds.max_percent).expect("range"),
                    bounds.step_percent,
                )
                .expect("valid bounds"),
            ),
        )
        .expect("valid domain charge limit");
        assert_eq!(limit.percent.map(|p| p.get()), Some(80));
    }
}
