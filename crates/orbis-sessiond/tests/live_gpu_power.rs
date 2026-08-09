//! Opt-in LIVE integration test для `SupergfxdGpuPowerProvider`.
//!
//! Тест помечен `#[ignore]` и запускается только явной командой:
//! ```text
//! cargo test -p orbis-sessiond live_gpu_power_provider_matches_supergfxd -- --ignored --exact --nocapture
//! ```
//!
//! Обычный `cargo test --workspace` НЕ читает live supergfxd/system D-Bus.
//!
//! Read-only: только вызов `org.supergfxctl.Daemon.Power()` и чтение PCI
//! `runtime_status` как supporting evidence; никаких SetMode/SetConfig,
//! никаких hardware writes.
//!
//! Если supergfxd недоступен на другой машине — тест падает с понятным
//! сообщением; это не превращается в production assumption.

use orbis_core::gpu::GpuPowerState;
use orbis_providers::error::ProviderError;
use orbis_providers::traits::GpuProvider;
use orbis_sessiond::supergfxd::{SupergfxdGpuPowerProvider, ZbusSupergfxdGpuPowerSource};

/// Прочитать raw `Power()` независимо от provider (read-only D-Bus call).
async fn raw_supergfxd_power(connection: &zbus::Connection) -> Result<u32, String> {
    // Минимальный inline proxy — только метод Power().
    #[zbus::proxy(
        interface = "org.supergfxctl.Daemon",
        default_service = "org.supergfxctl.Daemon",
        default_path = "/org/supergfxctl/Gfx"
    )]
    trait SupergfxdDaemonRaw {
        fn power(&self) -> zbus::Result<u32>;
    }
    let proxy = SupergfxdDaemonRawProxy::builder(connection)
        .build()
        .await
        .map_err(|e| format!("не удалось создать supergfxd proxy: {e}"))?;
    proxy
        .power()
        .await
        .map_err(|e| format!("не удалось вызвать Power(): {e}"))
}

#[tokio::test]
#[ignore = "requires live supergfxd system D-Bus (opt-in)"]
async fn live_gpu_power_provider_matches_supergfxd() {
    // Raw read независимо от provider.
    let connection = zbus::Connection::system()
        .await
        .unwrap_or_else(|e| panic!("не удалось подключиться к system bus: {e}"));
    let raw_power = raw_supergfxd_power(&connection)
        .await
        .unwrap_or_else(|e| panic!("{e}"));

    // Настоящий production source + provider.
    let source = ZbusSupergfxdGpuPowerSource::new(connection);
    let provider = SupergfxdGpuPowerProvider::new(source);

    let actual = provider
        .power_state()
        .await
        .unwrap_or_else(|e| panic!("provider power_state error: {e}"));

    // Canonical expected: PROVEN enum 0=Active,1=Suspended,2=Off, 3/4/other→Unknown.
    let expected = match raw_power {
        0 => GpuPowerState::Active,
        1 => GpuPowerState::Suspended,
        2 => GpuPowerState::Off,
        _ => GpuPowerState::Unknown,
    };

    assert_eq!(actual, expected, "provider power != canonical raw mapping");

    // Supporting evidence: PCI runtime_status (read-only, не часть contract).
    let status = std::fs::read_to_string("/sys/bus/pci/devices/0000:01:00.0/power/runtime_status")
        .map(|s| s.trim().to_string());
    match status {
        Ok(s) => eprintln!("supporting PCI runtime_status = '{s}'"),
        Err(_) => eprintln!("supporting PCI runtime_status недоступен (не failure)"),
    }

    // sanity: power_state доступен, а requested/mux/access честно Unsupported.
    assert!(matches!(
        provider.requested_mode().await,
        Err(ProviderError::Unsupported(_))
    ));
}
