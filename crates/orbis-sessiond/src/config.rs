//! Daemon-side product policy для sessiond.

/// Политика диапазона charge-limit (daemon-side product policy).
///
/// Это НЕ UPower-reported hardware capability: UPower не предоставляет
/// универсальные min/max/step. Значения согласованы с текущим UI-диапазоном
/// и централизованы здесь как единственный production владелец default
/// bounds; позднее их можно заменить конфигурацией или backend capability.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_is_40_100_5() {
        let config = SessiondConfig::default();
        assert_eq!(config.charge_limit.min_percent, 40);
        assert_eq!(config.charge_limit.max_percent, 100);
        assert_eq!(config.charge_limit.step_percent, 5);
    }
}
