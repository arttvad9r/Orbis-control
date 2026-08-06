//! # Orbis Test Support
//!
//! Mock-профили устройств и тестовые утилиты (Этап 2).
//!
//! Каждый профиль определяет: capabilities, текущие значения, задержки операций,
//! успешные операции/ошибки, requirement, pending state, события, телеметрию,
//! число вентиляторов, доступные power fields, GPU topology, display modes,
//! lighting, battery, версии backend-ов.
//!
//! `--mock-device <profile>` / `ORBIS_MOCK_DEVICE` выбирают профиль; в production
//! mock недоступен без явного флага.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod devices;

pub use devices::{MockDeviceProfile, ProfileName, all_profiles, build_state, profile_by_name};
