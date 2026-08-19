//! # Orbis Capabilities
//!
//! Capability-движок: построение матрицы возможностей устройства, сравнение
//! с эталонными фикстурами (snapshot-тесты), проверка приватности фикстур.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod diagnostics;
pub mod engine;
pub mod fixture;
pub mod probe;
pub mod registry;

pub use diagnostics::capability_snapshot_diagnostics;
pub use engine::{ProbeReport, build_from_parts};
pub use fixture::{
    ExpectedCapabilitiesFixture, ExpectedFeature, parse_status, privacy_check_fixture_dir,
};
pub use probe::{
    ProbeClassification, ProbeContext, ProbeError, ProbeOperationResult,
    capability_from_operations, classify_dbus_detail, resolve_overall_status,
};
pub use registry::{CapabilityRegistryBuilder, CapabilityRegistrySnapshot, RegistryError};
