//! Состояние дисплея.

use serde::{Deserialize, Serialize};

use crate::newtypes::RefreshHz;

/// Режим обновления экрана.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefreshMode {
    /// Частота, Гц.
    pub hz: RefreshHz,
}

impl RefreshMode {
    /// Конструктор.
    pub fn new(hz: RefreshHz) -> Self {
        Self { hz }
    }
}

/// Состояние Panel Overdrive (бинарная ASUS firmware-настройка).
///
/// Backend-семантика (kernel `asus-armoury`, sysfs
/// `panel_overdrive/current_value`): `0` = выключено, `1` = включено.
/// Отсутствие чтения не подменяется значением `false`: неизвестное
/// состояние представлено отдельным вариантом [`PanelOverdriveState::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelOverdriveState {
    /// Overdrive выключен (0).
    Disabled,
    /// Overdrive включён (1).
    Enabled,
    /// Backend не предоставил определённое состояние (например, malformed value).
    Unknown,
}

/// Состояние HDR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HdrState {
    /// Неизвестно.
    Unknown,
    /// HDR поддерживается экраном.
    Supported,
    /// HDR выключен.
    Disabled,
    /// HDR включён (функции, несовместимые с HDR, отключаются).
    Enabled,
}

/// Raw значение MiniLED mode (индекс kernel firmware-attributes enumeration).
///
/// Сохраняется losslessly; `u32` гарантирует, что будущий firmware enum
/// (например, новое поколение с `possible_values` больше `0;1;2`) не ломает
/// protocol/domain: неизвестные значения остаются raw и не превращаются в
/// default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MiniLedModeValue(u32);

impl MiniLedModeValue {
    /// Обернуть raw значение без предположений о семантике.
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// Raw индекс из kernel enumeration.
    pub const fn raw(self) -> u32 {
        self.0
    }
}

impl From<u32> for MiniLedModeValue {
    fn from(raw: u32) -> Self {
        Self::new(raw)
    }
}

impl From<MiniLedModeValue> for u32 {
    fn from(value: MiniLedModeValue) -> Self {
        value.0
    }
}

/// Известная product-level семантика значения MiniLED.
///
/// Метки выводятся из authoritative `possible_values` конкретного устройства
/// и задокументированных kernel maps, а не предполагаются глобально:
///
/// - allowed `[0,1]` (gen1 `mini_led_mode1_map`): `0`=Off, `1`=On;
/// - allowed `[0,1,2]` (gen2 `mini_led_mode2_map`): `0`=Off, `1`=Weak,
///   `2`=Strong;
/// - любое другое allowed set → семантика неизвестна, метки не выводятся.
///
/// Один и тот же индекс (`1`) имеет разное значение между поколениями
/// (gen1 On vs gen2 Weak), поэтому label вычисляется только вместе с allowed
/// set, никогда по raw в одиночку.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MiniLedModeKind {
    /// Выключено.
    Off,
    /// Включено (только поколение 1, allowed `[0,1]`).
    On,
    /// Слабая подсветка (только поколение 2, allowed `[0,1,2]`).
    Weak,
    /// Усиленная подсветка (только поколение 2, allowed `[0,1,2]`).
    Strong,
}

/// Полное состояние MiniLED mode capability (read-only snapshot).
///
/// - `allowed` — authoritative device-specific allowed set из `possible_values`
///   (порядок upstream сохраняется; сортировка не изобретается);
/// - `current` — fresh raw значение из `current_value`;
/// - `semantics` — опциональная доказанная метка текущего значения
///   ([`interpret_mini_led_mode`]); `None` = поколение/семантика неизвестны.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MiniLedModeState {
    /// Authoritative allowed raw values (device-specific).
    pub allowed: Vec<MiniLedModeValue>,
    /// Текущий raw value (fresh authoritative read).
    pub current: MiniLedModeValue,
    /// Опциональная доказанная семантика текущего значения.
    pub semantics: Option<MiniLedModeKind>,
}

/// Вывести известную семантику значения по authoritative allowed set.
///
/// Метки применяются только когда allowed set точно совпадает с одной из
/// задокументированных kernel maps. Для любого другого набора возвращается
/// `None` — генерация/семантика неизвестна, метки не выдумываются.
pub fn interpret_mini_led_mode(
    allowed: &[MiniLedModeValue],
    current: MiniLedModeValue,
) -> Option<MiniLedModeKind> {
    match allowed {
        [zero, one] if zero.raw() == 0 && one.raw() == 1 => match current.raw() {
            0 => Some(MiniLedModeKind::Off),
            1 => Some(MiniLedModeKind::On),
            _ => None,
        },
        [zero, one, two] if zero.raw() == 0 && one.raw() == 1 && two.raw() == 2 => {
            match current.raw() {
                0 => Some(MiniLedModeKind::Off),
                1 => Some(MiniLedModeKind::Weak),
                2 => Some(MiniLedModeKind::Strong),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Состояние дисплея, агрегированное для UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayMode {
    /// Текущая частота внутреннего дисплея (если известна).
    pub current_hz: Option<RefreshHz>,
    /// Доступные частоты.
    pub modes: Vec<RefreshMode>,
    /// Panel Overdrive (asusd/asus-armoury), если доступен.
    pub overdrive: PanelOverdriveState,
    /// Состояние HDR.
    pub hdr: HdrState,
}

impl Default for DisplayMode {
    fn default() -> Self {
        Self {
            current_hz: None,
            modes: Vec::new(),
            overdrive: PanelOverdriveState::Unknown,
            hdr: HdrState::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_default() {
        let d = DisplayMode::default();
        assert!(d.current_hz.is_none());
        assert!(d.modes.is_empty());
        assert_eq!(d.overdrive, PanelOverdriveState::Unknown);
        assert_eq!(d.hdr, HdrState::Unknown);
    }

    #[test]
    fn panel_overdrive_state_preserves_disabled_unknown_distinction() {
        assert_ne!(PanelOverdriveState::Disabled, PanelOverdriveState::Unknown);
        assert_ne!(PanelOverdriveState::Enabled, PanelOverdriveState::Unknown);

        let json = serde_json::to_string(&PanelOverdriveState::Disabled).unwrap();
        assert_eq!(json, "\"disabled\"");
        let back: PanelOverdriveState = serde_json::from_str(&json).unwrap();
        assert_eq!(back, PanelOverdriveState::Disabled);
    }

    #[test]
    fn display_with_modes() {
        let d = DisplayMode {
            current_hz: Some(RefreshHz::new(165).unwrap()),
            modes: vec![
                RefreshMode::new(RefreshHz::new(60).unwrap()),
                RefreshMode::new(RefreshHz::new(165).unwrap()),
            ],
            overdrive: PanelOverdriveState::Enabled,
            hdr: HdrState::Disabled,
        };
        assert_eq!(d.modes.len(), 2);
    }

    fn values(raws: &[u32]) -> Vec<MiniLedModeValue> {
        raws.iter().copied().map(MiniLedModeValue::new).collect()
    }

    #[test]
    fn mini_led_raw_values_round_trip_losslessly() {
        for raw in [0u32, 1, 2, 3, 255, u32::MAX] {
            let value = MiniLedModeValue::new(raw);
            assert_eq!(u32::from(value), raw);
            assert_eq!(value.raw(), raw);
        }
    }

    #[test]
    fn mini_led_unknown_future_value_round_trips() {
        // Будущий firmware enum (например, 3) не ломает domain: raw сохраняется.
        let future = MiniLedModeValue::new(3);
        assert_eq!(future.raw(), 3);
        let json = serde_json::to_string(&future).unwrap();
        assert_eq!(json, "3");
        let back: MiniLedModeValue = serde_json::from_str(&json).unwrap();
        assert_eq!(back, future);
    }

    #[test]
    fn mini_led_interpretation_matches_documented_kernel_maps() {
        // gen1: [0,1] -> Off/On.
        assert_eq!(
            interpret_mini_led_mode(&values(&[0, 1]), MiniLedModeValue::new(0)),
            Some(MiniLedModeKind::Off)
        );
        assert_eq!(
            interpret_mini_led_mode(&values(&[0, 1]), MiniLedModeValue::new(1)),
            Some(MiniLedModeKind::On)
        );
        // gen2: [0,1,2] -> Off/Weak/Strong.
        assert_eq!(
            interpret_mini_led_mode(&values(&[0, 1, 2]), MiniLedModeValue::new(0)),
            Some(MiniLedModeKind::Off)
        );
        assert_eq!(
            interpret_mini_led_mode(&values(&[0, 1, 2]), MiniLedModeValue::new(1)),
            Some(MiniLedModeKind::Weak)
        );
        assert_eq!(
            interpret_mini_led_mode(&values(&[0, 1, 2]), MiniLedModeValue::new(2)),
            Some(MiniLedModeKind::Strong)
        );
    }

    #[test]
    fn mini_led_interpretation_is_generation_gated_not_raw_global() {
        // Один и тот же raw 1 имеет РАЗНУЮ метку между поколениями — label
        // существует только вместе с matching allowed set.
        let gen1 = interpret_mini_led_mode(&values(&[0, 1]), MiniLedModeValue::new(1));
        let gen2 = interpret_mini_led_mode(&values(&[0, 1, 2]), MiniLedModeValue::new(1));
        assert_eq!(gen1, Some(MiniLedModeKind::On));
        assert_eq!(gen2, Some(MiniLedModeKind::Weak));
        assert_ne!(gen1, gen2);
    }

    #[test]
    fn mini_led_unknown_generation_gets_no_labels() {
        // Future/unknown allowed set (например, [0,1,2,3]) — метки не выводятся.
        assert_eq!(
            interpret_mini_led_mode(&values(&[0, 1, 2, 3]), MiniLedModeValue::new(0)),
            None
        );
        assert_eq!(
            interpret_mini_led_mode(&values(&[0, 1, 2, 3]), MiniLedModeValue::new(1)),
            None
        );
        // Нестандартный порядок/набор — метки не выводятся.
        assert_eq!(
            interpret_mini_led_mode(&values(&[1, 0]), MiniLedModeValue::new(0)),
            None
        );
        assert_eq!(
            interpret_mini_led_mode(&values(&[0, 2, 1]), MiniLedModeValue::new(1)),
            None
        );
        // Empty allowed set — метки не выводятся.
        assert_eq!(interpret_mini_led_mode(&[], MiniLedModeValue::new(0)), None);
    }

    #[test]
    fn mini_led_current_outside_allowed_set_gets_no_label() {
        // current вне allowed set — это уже error на provider уровне; если бы
        // state был построен, label всё равно отсутствовал бы.
        assert_eq!(
            interpret_mini_led_mode(&values(&[0, 1]), MiniLedModeValue::new(2)),
            None
        );
        assert_eq!(
            interpret_mini_led_mode(&values(&[0, 1, 2]), MiniLedModeValue::new(5)),
            None
        );
    }

    #[test]
    fn mini_led_state_serialization_is_raw_preserving() {
        let state = MiniLedModeState {
            allowed: values(&[0, 1]),
            current: MiniLedModeValue::new(1),
            semantics: Some(MiniLedModeKind::On),
        };
        let json = serde_json::to_string(&state).unwrap();
        let back: MiniLedModeState = serde_json::from_str(&json).unwrap();
        assert_eq!(back, state);
    }
}
