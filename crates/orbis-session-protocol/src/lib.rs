//! # Orbis Session Protocol
//!
//! Нейтральный read-only D-Bus wire-контракт между `orbis-session-client`
//! (GUI-side D-Bus client) и `orbis-sessiond` (user session daemon).
//!
//! - контракт содержит только чтение Battery Charge Limit;
//! - crate не создаёт D-Bus connection, runtime и не обращается к hardware;
//! - версия интерфейса зафиксирована в имени `Session1`.
//!
//! Запрещено добавлять в этот crate mutation API (getter-only контракт)
//! и зависимости от доменного/runtime слоя.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Стабильное имя D-Bus service (system/session bus).
pub const BUS_NAME: &str = "io.github.orbiscontrol.Session";

/// Стабильный object path D-Bus service.
pub const OBJECT_PATH: &str = "/io/github/orbiscontrol/Session";

/// Имя интерфейса D-Bus (версия интерфейса закодирована как `1`).
pub const INTERFACE_NAME: &str = "io.github.orbiscontrol.Session1";

/// Wire DTO Battery Charge Limit.
///
/// Представление явное и стабильное: D-Bus не имеет универсального нативного
/// `Option<u8>`, поэтому достоверность `percent` выражается отдельным флагом
/// `percent_present`.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    zbus::zvariant::Type,
    zbus::zvariant::OwnedValue,
)]
pub struct ChargeLimitInfo {
    /// Активна ли функция ограничения заряда.
    pub enabled: bool,
    /// Достоверно ли текущее значение `percent`.
    pub percent_present: bool,
    /// Текущий процент; при `percent_present == false` равен 0.
    pub percent: u8,
    /// Известны ли hardware/backend constraints (min/max/step).
    pub bounds_present: bool,
    /// Нижняя поддерживаемая граница; при `bounds_present == false` равен 0.
    pub min_percent: u8,
    /// Верхняя поддерживаемая граница; при `bounds_present == false` равен 0.
    pub max_percent: u8,
    /// Минимальный объявленный шаг backend; при `bounds_present == false` равен 0.
    pub step_percent: u8,
}

impl ChargeLimitInfo {
    /// Создать DTO с достоверным текущим значением процента и известными bounds.
    ///
    /// Диапазон/шаг не валидируются здесь: проверка согласованности
    /// min/max/step выполняется на границе конкретного backend/domain
    /// conversion (protocol crate не считает диапазон 40..=100 универсальным).
    pub const fn with_percent(
        enabled: bool,
        percent: u8,
        min_percent: u8,
        max_percent: u8,
        step_percent: u8,
    ) -> Self {
        Self {
            enabled,
            percent_present: true,
            percent,
            bounds_present: true,
            min_percent,
            max_percent,
            step_percent,
        }
    }

    /// Создать DTO с достоверным текущим значением процента и неизвестными
    /// bounds (`bounds_present = false`, min/max/step = 0).
    pub const fn with_percent_unknown_bounds(enabled: bool, percent: u8) -> Self {
        Self {
            enabled,
            percent_present: true,
            percent,
            bounds_present: false,
            min_percent: 0,
            max_percent: 0,
            step_percent: 0,
        }
    }

    /// Создать DTO без достоверного значения процента и с известными bounds.
    ///
    /// Устанавливает `percent_present = false` и `percent = 0`.
    pub const fn without_percent(
        enabled: bool,
        min_percent: u8,
        max_percent: u8,
        step_percent: u8,
    ) -> Self {
        Self {
            enabled,
            percent_present: false,
            percent: 0,
            bounds_present: true,
            min_percent,
            max_percent,
            step_percent,
        }
    }

    /// Создать DTO без достоверного значения процента и с неизвестными bounds.
    ///
    /// Устанавливает `percent_present = false`, `percent = 0`,
    /// `bounds_present = false`, min/max/step = 0.
    pub const fn without_percent_unknown_bounds(enabled: bool) -> Self {
        Self {
            enabled,
            percent_present: false,
            percent: 0,
            bounds_present: false,
            min_percent: 0,
            max_percent: 0,
            step_percent: 0,
        }
    }

    /// Текущий процент, если он достоверен.
    pub const fn percent(self) -> Option<u8> {
        if self.percent_present {
            Some(self.percent)
        } else {
            None
        }
    }
}

/// Getter-only zbus proxy контракт интерфейса `Session1`.
///
/// Контракт read-only: только чтение свойства `ChargeLimit`; setter,
/// mutation-методы и signals в этом микрошаге отсутствуют.
#[zbus::proxy(
    interface = "io.github.orbiscontrol.Session1",
    default_service = "io.github.orbiscontrol.Session",
    default_path = "/io/github/orbiscontrol/Session"
)]
pub trait Session1 {
    /// Текущий Battery Charge Limit (read-only property).
    #[zbus(property)]
    fn charge_limit(&self) -> zbus::Result<ChargeLimitInfo>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::Type;

    #[test]
    fn dbus_names_are_stable() {
        assert_eq!(BUS_NAME, "io.github.orbiscontrol.Session");
        assert_eq!(OBJECT_PATH, "/io/github/orbiscontrol/Session");
        assert_eq!(INTERFACE_NAME, "io.github.orbiscontrol.Session1");
    }

    #[test]
    fn charge_limit_with_percent() {
        let info = ChargeLimitInfo::with_percent(true, 80, 40, 100, 5);
        assert!(info.enabled);
        assert!(info.percent_present);
        assert_eq!(info.percent, 80);
        assert_eq!(info.percent(), Some(80));
        assert!(info.bounds_present);
        assert_eq!(info.min_percent, 40);
        assert_eq!(info.max_percent, 100);
        assert_eq!(info.step_percent, 5);
    }

    #[test]
    fn charge_limit_with_percent_unknown_bounds() {
        let info = ChargeLimitInfo::with_percent_unknown_bounds(true, 80);
        assert!(info.enabled);
        assert!(info.percent_present);
        assert_eq!(info.percent, 80);
        assert_eq!(info.percent(), Some(80));
        assert!(!info.bounds_present);
        assert_eq!(info.min_percent, 0);
        assert_eq!(info.max_percent, 0);
        assert_eq!(info.step_percent, 0);
    }

    #[test]
    fn charge_limit_without_percent() {
        let info = ChargeLimitInfo::without_percent(false, 40, 100, 5);
        assert!(!info.enabled);
        assert!(!info.percent_present);
        assert_eq!(info.percent, 0);
        assert_eq!(info.percent(), None);
        assert!(info.bounds_present);
        assert_eq!(info.min_percent, 40);
        assert_eq!(info.max_percent, 100);
        assert_eq!(info.step_percent, 5);
    }

    #[test]
    fn charge_limit_without_percent_unknown_bounds() {
        let info = ChargeLimitInfo::without_percent_unknown_bounds(false);
        assert!(!info.enabled);
        assert!(!info.percent_present);
        assert_eq!(info.percent, 0);
        assert_eq!(info.percent(), None);
        assert!(!info.bounds_present);
        assert_eq!(info.min_percent, 0);
        assert_eq!(info.max_percent, 0);
        assert_eq!(info.step_percent, 0);
    }

    #[test]
    fn charge_limit_dbus_signature_is_stable() {
        // bool(bool)u8 bool u8 u8 u8 -> "(bbybyyy)"
        let expected: zbus::zvariant::Signature = "(bbybyyy)".try_into().expect("valid signature");
        assert_eq!(*ChargeLimitInfo::SIGNATURE, expected);
    }

    #[test]
    fn charge_limit_serde_roundtrip() {
        let info = ChargeLimitInfo::with_percent(true, 80, 40, 100, 5);
        let ctx = zbus::zvariant::serialized::Context::new_dbus(zbus::zvariant::Endian::Little, 0);
        let data = zbus::zvariant::to_bytes(ctx, &info).expect("serialize");
        let (decoded, _): (ChargeLimitInfo, usize) = data.deserialize().expect("deserialize");
        assert_eq!(decoded, info);
    }
}
