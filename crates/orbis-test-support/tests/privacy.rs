//! Privacy-тест обезличенных аппаратных фикстур.
//!
//! Проверяет, что фикстуры `tests/fixtures/hardware/fa707nv/` не содержат
//! персональных данных (серийники, hostname, MAC, UUID, домашние пути и т.п.).

use std::path::Path;

/// Каталог фикстур относительно корня workspace.
fn fixtures_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/hardware/fa707nv")
}

#[test]
fn fixtures_are_anonymized() {
    let dir = fixtures_dir();
    assert!(dir.is_dir(), "нет каталога фикстур: {}", dir.display());

    let problems = orbis_capabilities::privacy_check_fixture_dir(&dir);
    assert!(
        problems.is_empty(),
        "фикстуры содержат персональные данные:\n{}",
        problems.join("\n")
    );
}

#[test]
fn expected_capabilities_parses() {
    let path = fixtures_dir().join("expected-capabilities.json");
    let fixture = orbis_capabilities::ExpectedCapabilitiesFixture::load(&path)
        .expect("expected-capabilities.json должен парситься");
    let caps = fixture
        .to_device_capabilities()
        .expect("capability-матрица");
    // Ключевые статусы из реального probe FA707NV.
    assert_eq!(
        caps.status(orbis_core::capability::FeatureId::GpuMux),
        orbis_core::capability::CapabilityStatus::SupportedWithRequirement
    );
    assert_eq!(
        caps.status(orbis_core::capability::FeatureId::PptPl1Spl),
        orbis_core::capability::CapabilityStatus::ReadOnly
    );
    assert_eq!(
        caps.status(orbis_core::capability::FeatureId::CpuBoost),
        orbis_core::capability::CapabilityStatus::PermissionDenied
    );
}

#[test]
fn fixture_has_no_forbidden_patterns_in_raw_files() {
    // Дублирующая проверка по сырым файлам (XML/JSON/TOML), независимо от
    // privacy_check_fixture_dir.
    let dir = fixtures_dir();
    let forbidden = ["serial", "hostname", "machine-id", "/home/", "mac="];
    for entry in std::fs::read_dir(&dir).unwrap().flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let content = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .to_lowercase();
        for pat in forbidden {
            assert!(
                !content.contains(pat),
                "{} содержит запрещённый паттерн '{}'",
                path.display(),
                pat
            );
        }
    }
}
