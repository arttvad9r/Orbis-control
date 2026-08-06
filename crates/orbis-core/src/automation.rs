//! Правила автоматизации.

use serde::{Deserialize, Serialize};

use crate::gpu::GpuMode;
use crate::newtypes::RefreshHz;
use crate::profile::PerformanceProfile;

/// Триггер правила автоматизации.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationTrigger {
    /// Переход на AC.
    OnAc,
    /// Переход на батарею.
    OnBattery,
    /// Слабое USB-C зарядное.
    OnUsbCPdLowPower,
    /// Подключение внешнего дисплея.
    ExternalDisplayConnected,
    /// Отключение внешнего дисплея.
    ExternalDisplayDisconnected,
    /// Выход из сна (resume).
    OnResume,
    /// Закрытие крышки (только через logind).
    LidClosed,
}

/// Действие правила автоматизации.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationAction {
    /// Сменить профиль производительности.
    SetProfile(PerformanceProfile),
    /// Сменить GPU-политику.
    SetGpuPolicy(GpuMode),
    /// Сменить политику частоты экрана (минимум/максимум/auto).
    SetRefreshPolicy(RefreshPolicy),
    /// Выключить/включить подсветку.
    SetLighting(bool),
    /// Пользовательская команда (список аргументов; выполняется только с
    /// явного разрешения пользователя, без /bin/sh -c).
    CustomCommand(Vec<String>),
}

/// Политика частоты обновления экрана.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshPolicy {
    /// Минимальная доступная частота (батарея).
    Minimum,
    /// Максимальная доступная частота.
    Maximum,
    /// Авто.
    Auto,
    /// Конкретная частота.
    Fixed(RefreshHz),
}

/// Правило автоматизации.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationRule {
    /// Уникальный идентификатор.
    pub id: String,
    /// Триггер.
    pub trigger: AutomationTrigger,
    /// Действие.
    pub action: AutomationAction,
    /// Приоритет (больше = важнее).
    pub priority: u8,
    /// Cooldown между применениями, мс.
    pub cooldown_ms: u64,
    /// Правило включено.
    pub enabled: bool,
}

impl AutomationRule {
    /// Новое правило по умолчанию (cooldown 1500 мс, включено).
    pub fn new(id: impl Into<String>, trigger: AutomationTrigger, action: AutomationAction, priority: u8) -> Self {
        Self { id: id.into(), trigger, action, priority, cooldown_ms: 1500, enabled: true }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_rule() {
        let r = AutomationRule::new(
            "ac-profile",
            AutomationTrigger::OnAc,
            AutomationAction::SetProfile(PerformanceProfile::Balanced),
            10,
        );
        assert!(r.enabled);
        assert_eq!(r.cooldown_ms, 1500);
    }

    #[test]
    fn refresh_policy_serde() {
        let p = RefreshPolicy::Fixed(RefreshHz::new(60).unwrap());
        let json = serde_json::to_string(&p).unwrap();
        let back: RefreshPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
    }
}
