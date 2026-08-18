//! Read-only session-side display output state (Wayland/compositor concern).
//!
//! Это НЕ ASUS firmware capability. Это состояние, которое compositor
//! (Wayland) реально показывает: outputs и их current mode/refresh.
//!
//! Модель построена строго по core `wl_output` protocol (wayland.xml):
//! - `wl_output.mode`: `flags` (bitfield `current`=0x1, `preferred`=0x2),
//!   `width`/`height` в hardware units, `refresh` в mHz;
//! - «there will always be one mode, the current mode»; «the current mode is
//!   always the last mode that was received with the current flag set»;
//! - «Non-current modes are deprecated. A compositor can decide to only
//!   advertise the current mode and never send other modes. Clients should not
//!   rely on non-current modes»;
//! - refresh может быть 0 «if it doesn't make sense for this output (e.g. for
//!   virtual outputs)»;
//! - `wl_output.name` (v4+): уникален только для compositor instance, НЕ
//!   persistent across sessions, не гарантирует отражение DRM connector;
//! - `wl_output.done` (v2+): атомарная граница набора событий.

use serde::{Deserialize, Serialize};

use crate::newtypes::RefreshMilliHz;

/// Идентичность output в рамках текущего compositor instance.
///
/// `wl_output.name` (v4+) — user-friendly имя, уникальное среди всех
/// `wl_output` globals данного compositor instance. НЕ гарантировано
/// persistent across sessions и не обязано отражать DRM connector
/// (protocol: «do not assume that the name is a reflection of an underlying
/// DRM connector»). Поэтому это runtime identity, а не стабильный ключ для
/// конфигурации.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DisplayOutputId(String);

impl DisplayOutputId {
    /// Создать identity из compositor-provided name.
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// Имя output (compositor-provided).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for DisplayOutputId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Один mode output-а (наблюдение compositor-а).
///
/// `width`/`height` — в hardware units (не compositor space; для logical size
/// используется xdg_output). `refresh` — в mHz, lossless.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayMode {
    /// Ширина mode в hardware units.
    pub width: u32,
    /// Высота mode в hardware units.
    pub height: u32,
    /// Вертикальная частота в mHz (0 = не имеет смысла для этого output).
    pub refresh: RefreshMilliHz,
}

impl DisplayMode {
    /// Создать mode.
    pub fn new(width: u32, height: u32, refresh: RefreshMilliHz) -> Self {
        Self {
            width,
            height,
            refresh,
        }
    }
}

/// Текущий mode output-а.
///
/// `current` — mode, помеченный compositor-ом флагом `current` (последний
/// полученный с этим флагом). `preferred` — mode, помеченный флагом
/// `preferred` (если compositor его прислал); НЕ равен current автоматически.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentDisplayMode {
    /// Текущий mode (authoritative, из флага `current`).
    pub current: DisplayMode,
    /// Preferred mode, если compositor его прислал (не обязан быть current).
    pub preferred: Option<DisplayMode>,
}

/// Полное read-only состояние одного output-а.
///
/// - `id` — runtime identity (compositor-provided name, если доступен);
/// - `current_mode` — текущий mode + optional preferred;
/// - `available_modes` — optional наблюдение non-current modes, если
///   compositor их прислал. Отсутствие списка НЕ ошибка и НЕ Unsupported:
///   core `wl_output` не гарантирует полную enumeration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayOutputState {
    /// Runtime identity output-а.
    pub id: DisplayOutputId,
    /// Текущий mode (authoritative).
    pub current_mode: CurrentDisplayMode,
    /// Optional наблюдение non-current modes (не обязательная часть контракта).
    pub available_modes: Vec<DisplayMode>,
}

/// Read-only snapshot текущих outputs compositor-а.
///
/// Пустой список означает «compositor сейчас не показывает ни одного
/// физического output» — НЕ создаётся fake internal panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayOutputSnapshot {
    /// Outputs, которые compositor реально показывает.
    pub outputs: Vec<DisplayOutputState>,
}

impl DisplayOutputSnapshot {
    /// Пустой snapshot (нет outputs).
    pub fn empty() -> Self {
        Self {
            outputs: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(w: u32, h: u32, mhz: u32) -> DisplayMode {
        DisplayMode::new(w, h, RefreshMilliHz::new(mhz).unwrap())
    }

    #[test]
    fn output_id_roundtrip() {
        let id = DisplayOutputId::new("eDP-1");
        assert_eq!(id.as_str(), "eDP-1");
        assert_eq!(id.to_string(), "eDP-1");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"eDP-1\"");
        let back: DisplayOutputId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn current_mode_preferred_does_not_imply_current() {
        // preferred flag не означает current: они независимы.
        let state = CurrentDisplayMode {
            current: mode(1920, 1080, 60000),
            preferred: Some(mode(1920, 1080, 120000)),
        };
        assert_eq!(state.current.refresh.get(), 60000);
        assert_eq!(state.preferred.unwrap().refresh.get(), 120000);
        assert_ne!(state.current, state.preferred.unwrap());
    }

    #[test]
    fn refresh_precision_preserved_in_mode() {
        // 59.94 Hz не округляется до 60.
        let m = mode(1920, 1080, 59940);
        assert_eq!(m.refresh.get(), 59940);
        assert_ne!(m.refresh.get(), 60000);
    }

    #[test]
    fn refresh_zero_is_not_meaningful_not_zero_hz() {
        // 0 допустим (virtual output), не становится 60 Hz.
        let m = mode(1920, 1080, 0);
        assert_eq!(m.refresh.get(), 0);
    }

    #[test]
    fn snapshot_empty_has_no_fake_output() {
        let s = DisplayOutputSnapshot::empty();
        assert!(s.outputs.is_empty());
    }

    #[test]
    fn output_state_serialization_roundtrip() {
        let state = DisplayOutputState {
            id: DisplayOutputId::new("eDP-1"),
            current_mode: CurrentDisplayMode {
                current: mode(2560, 1600, 120000),
                preferred: Some(mode(2560, 1600, 120000)),
            },
            available_modes: vec![mode(2560, 1600, 60000), mode(2560, 1600, 120000)],
        };
        let json = serde_json::to_string(&state).unwrap();
        let back: DisplayOutputState = serde_json::from_str(&json).unwrap();
        assert_eq!(back, state);
    }
}
