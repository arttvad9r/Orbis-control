//! Opt-in LIVE integration test для `KernelPerformanceProvider`.
//!
//! Тест помечен `#[ignore]` и запускается только явной командой:
//! ```text
//! cargo test -p orbis-sessiond live_performance_provider_matches_kernel_sysfs -- --ignored --exact --nocapture
//! ```
//!
//! Обычный `cargo test --workspace` НЕ читает live hardware/sysfs.
//!
//! Требования:
//! - наличие `/sys/firmware/acpi/platform_profile`;
//! - наличие `/sys/firmware/acpi/platform_profile_choices`;
//! - read-only: никаких записей в эти файлы, никаких setters, никакого
//!   profile switching.
//!
//! Если ABI отсутствует на другой машине — тест падает с понятным сообщением;
//! это не превращается в production assumption.

use orbis_core::profile::PerformanceProfile;
use orbis_providers::traits::PerformanceProvider;
use orbis_sessiond::performance::{KernelPerformanceProvider, SysfsKernelPlatformProfileSource};

const CURRENT_PATH: &str = "/sys/firmware/acpi/platform_profile";
const CHOICES_PATH: &str = "/sys/firmware/acpi/platform_profile_choices";

/// Прочитать raw symbolic содержимое файла (trim).
fn read_raw(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .map_err(|e| format!("не удалось прочитать '{path}': {e}"))
}

#[tokio::test]
#[ignore = "requires live kernel platform_profile sysfs (opt-in)"]
async fn live_performance_provider_matches_kernel_sysfs() {
    // Raw read независимо от provider.
    let raw_current = match read_raw(CURRENT_PATH) {
        Ok(v) => v,
        Err(e) => panic!("kernel ABI отсутствует/недоступен: {e}"),
    };
    let raw_choices_raw = match read_raw(CHOICES_PATH) {
        Ok(v) => v,
        Err(e) => panic!("kernel ABI отсутствует/недоступен: {e}"),
    };
    let raw_choices: Vec<&str> = raw_choices_raw.split_whitespace().collect();

    // Canonical domain expected из raw symbolic значений (существующий contract).
    let expected_current = PerformanceProfile::parse(&raw_current)
        .unwrap_or_else(|e| panic!("raw current '{raw_current}' не маппится в domain: {e}"));
    let expected_available: Vec<PerformanceProfile> = raw_choices
        .iter()
        .map(|s| {
            PerformanceProfile::parse(s)
                .unwrap_or_else(|e| panic!("raw choice '{s}' не маппится в domain: {e}"))
        })
        .collect();

    // Настоящий production source + provider.
    let source = SysfsKernelPlatformProfileSource::default();
    let provider = KernelPerformanceProvider::new(source);

    let actual_current = provider
        .current_profile()
        .await
        .unwrap_or_else(|e| panic!("provider current_profile error: {e}"));
    let actual_available = provider
        .profiles()
        .await
        .unwrap_or_else(|e| panic!("provider profiles error: {e}"));

    // Current comparison.
    assert_eq!(
        actual_current, expected_current,
        "provider current != canonical parsed raw current"
    );

    // Available comparison. Domain contract не гарантирует порядок choices;
    // сравниваем как множества.
    let mut actual_sorted = actual_available.clone();
    let mut expected_sorted = expected_available.clone();
    actual_sorted.sort();
    expected_sorted.sort();
    assert_eq!(
        actual_sorted, expected_sorted,
        "provider available != canonical parsed raw choices"
    );
}
