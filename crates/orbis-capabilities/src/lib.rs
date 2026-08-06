//! # Orbis Capabilities
//!
//! Capability-движок: построение матрицы возможностей устройства, сравнение
//! с эталонными фикстурами (snapshot-тесты), проверка приватности фикстур.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod engine;
pub mod fixture;

pub use engine::{ProbeReport, build_from_parts};
pub use fixture::{
    ExpectedCapabilitiesFixture, ExpectedFeature, parse_status, privacy_check_fixture_dir,
};
