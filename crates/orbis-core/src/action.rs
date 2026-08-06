//! Операции: требования, результаты, pending-действия.

use serde::{Deserialize, Serialize};

/// Требование, связанное с операцией.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionRequirement {
    /// Требований нет.
    None,
    /// Требуется выход из сессии.
    Logout,
    /// Требуется перезагрузка.
    Reboot,
    /// Требуется подтверждение пользователя.
    Confirmation,
    /// Экспериментальная операция.
    Experimental,
}

impl ActionRequirement {
    /// Имя для диагностики.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Logout => "logout",
            Self::Reboot => "reboot",
            Self::Confirmation => "confirmation",
            Self::Experimental => "experimental",
        }
    }
}

/// Результат применения операции.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ApplyResult {
    /// Применено и подтверждено read-back.
    Applied,
    /// Применено, но требует reboot/logout (pending).
    Pending {
        /// требование
        requirement: ActionRequirement,
    },
    /// Не удалось применить.
    Failed {
        /// причина
        reason: String,
        /// backend
        backend: String,
    },
    /// Применено, затем откачено из-за частичной ошибки.
    RolledBack {
        /// причина отката
        reason: String,
    },
}

impl ApplyResult {
    /// Успех без pending?
    pub fn is_applied(&self) -> bool {
        matches!(self, Self::Applied)
    }

    /// Нужен ли reboot/logout?
    pub fn requirement(&self) -> Option<ActionRequirement> {
        match self {
            Self::Pending { requirement } => Some(*requirement),
            _ => None,
        }
    }
}

/// Pending-действие (например, MUX после reboot).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingAction {
    /// Уникальный идентификатор.
    pub id: String,
    /// Цель операции (например, "gpu_mux: ultimate").
    pub target: String,
    /// Требование.
    pub requirement: ActionRequirement,
    /// Отменяемо ли.
    pub cancelable: bool,
    /// Кем создано.
    pub created_by: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_result_helpers() {
        assert!(ApplyResult::Applied.is_applied());
        let pending = ApplyResult::Pending {
            requirement: ActionRequirement::Reboot,
        };
        assert!(!pending.is_applied());
        assert_eq!(pending.requirement(), Some(ActionRequirement::Reboot));
    }

    #[test]
    fn apply_result_serde() {
        let r = ApplyResult::Failed {
            reason: "ENODEV".into(),
            backend: "asusd".into(),
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: ApplyResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn requirement_names() {
        assert_eq!(ActionRequirement::Reboot.as_str(), "reboot");
        assert_eq!(ActionRequirement::Logout.as_str(), "logout");
    }
}
