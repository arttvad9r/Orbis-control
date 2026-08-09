//! Opt-in LIVE integration test для `ArmouryGpuProvider` (MUX + access).
//!
//! Тест помечен `#[ignore]` и запускается только явной командой:
//! ```text
//! cargo test -p orbis-sessiond live_gpu_mux_access_provider_matches_sysfs -- --ignored --exact --nocapture
//! ```
//!
//! Обычный `cargo test --workspace` НЕ читает live sysfs.
//!
//! Read-only: только чтение
//! `/sys/class/firmware-attributes/asus-armoury/attributes/{gpu_mux_mode,dgpu_disable}/current_value`;
//! никаких writes/setters/queued-операций.
//!
//! Если ASUS Armoury sysfs отсутствует на другой машине — тест падает с
//! понятным сообщением; это не превращается в production assumption.

use orbis_core::gpu::{GpuAccessPolicy, GpuMuxState};
use orbis_providers::traits::{GpuAccessProvider, GpuMuxProvider};
use orbis_sessiond::armoury::{ArmouryGpuProvider, SysfsArmouryGpuSource};

const MUX_PATH: &str =
    "/sys/class/firmware-attributes/asus-armoury/attributes/gpu_mux_mode/current_value";
const DGPU_PATH: &str =
    "/sys/class/firmware-attributes/asus-armoury/attributes/dgpu_disable/current_value";

/// Прочитать raw current_value независимо от provider.
fn read_raw(path: &str) -> Result<u32, String> {
    std::fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .map_err(|e| format!("не удалось прочитать '{path}': {e}"))?
        .parse::<u32>()
        .map_err(|e| format!("невалидное значение в '{path}': {e}"))
}

#[tokio::test]
#[ignore = "requires live asus-armoury firmware-attributes sysfs (opt-in)"]
async fn live_gpu_mux_access_provider_matches_sysfs() {
    // Raw read независимо от provider.
    let raw_mux =
        read_raw(MUX_PATH).unwrap_or_else(|e| panic!("kernel asus-armoury ABI недоступен: {e}"));
    let raw_dgpu =
        read_raw(DGPU_PATH).unwrap_or_else(|e| panic!("kernel asus-armoury ABI недоступен: {e}"));

    // Canonical expected из PROVEN kernel mapping.
    let expected_mux = match raw_mux {
        0 => GpuMuxState::Discrete,
        1 => GpuMuxState::Integrated,
        _ => GpuMuxState::Unknown,
    };
    let expected_access = match raw_dgpu {
        0 => GpuAccessPolicy::Unblocked,
        1 => GpuAccessPolicy::Blocked,
        _ => GpuAccessPolicy::Unknown,
    };

    // Настоящий production source + provider.
    let source = SysfsArmouryGpuSource::default();
    let provider = ArmouryGpuProvider::new(source);

    let actual_mux = provider
        .mux_state()
        .await
        .unwrap_or_else(|e| panic!("provider mux_state error: {e}"));
    let actual_access = provider
        .access_policy()
        .await
        .unwrap_or_else(|e| panic!("provider access_policy error: {e}"));

    eprintln!("raw mux={raw_mux} dgpu={raw_dgpu}");
    assert_eq!(
        actual_mux, expected_mux,
        "provider mux != canonical raw mapping"
    );
    assert_eq!(
        actual_access, expected_access,
        "provider access != canonical raw mapping"
    );
}
