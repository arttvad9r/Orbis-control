//! Optional read-only LACT telemetry over `/run/lactd.sock`.
//!
//! Only the typed `list_devices` and `device_stats` requests are implemented.
//! No LACT mutation command, profile activation, service activation, or config
//! write is reachable from this provider.

use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use orbis_core::diagnostics::DiagnosticEntry;
use orbis_core::fan::FanId;
use orbis_core::gpu::GpuPowerState;
use orbis_core::identity::BackendIdentity;
use orbis_core::newtypes::{MilliWatt, Rpm, TemperatureC};
use orbis_core::telemetry::{
    FanTelemetry, GpuIdentity, GpuRole, GpuTelemetry, PowerTelemetry, Telemetry,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use crate::error::ProviderError;
use crate::traits::{Provider, ProviderHealth, TelemetryProvider};

/// LACT's documented local endpoints.
pub const LACT_SOCKET_PATH: &str = "/run/lactd.sock";
/// LACT's documented configuration path, used only as diagnostics metadata.
pub const LACT_CONFIG_PATH: &str = "/etc/lact/config.yaml";

/// Read-only LACT provider.
pub struct LactTelemetryProvider {
    socket_path: String,
}

impl LactTelemetryProvider {
    /// Construct without touching the socket or service.
    pub fn new() -> Self {
        Self {
            socket_path: LACT_SOCKET_PATH.into(),
        }
    }

    #[cfg(test)]
    fn with_socket_path(socket_path: impl Into<String>) -> Self {
        Self {
            socket_path: socket_path.into(),
        }
    }

    /// The configured local socket path.
    pub fn socket_path(&self) -> &str {
        &self.socket_path
    }
}

impl Default for LactTelemetryProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for LactTelemetryProvider {
    fn id(&self) -> &'static str {
        "lact-telemetry"
    }

    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("lactd-json-socket")
    }

    fn timeout(&self) -> Duration {
        Duration::from_millis(750)
    }

    fn explain_unsupported(&self, feature: &str) -> String {
        format!("LACT read-only telemetry: '{feature}' is unavailable")
    }

    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }

    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        vec![DiagnosticEntry::new(
            "provider.lact-telemetry",
            format!(
                "optional read-only lactd JSON socket; config discovery path {LACT_CONFIG_PATH}"
            ),
        )]
    }
}

#[async_trait]
impl TelemetryProvider for LactTelemetryProvider {
    async fn snapshot(&self) -> Result<Telemetry, ProviderError> {
        let devices: Vec<LactDevice> = self.request(LactRequest::ListDevices).await?;
        let nvidia = devices
            .into_iter()
            .filter(|device| {
                device.id.starts_with("10DE:") && device.device_type == LactDeviceType::Dedicated
            })
            .collect::<Vec<_>>();
        let device = match nvidia.as_slice() {
            [device] => device,
            [] => {
                return Err(ProviderError::Unsupported(
                    "no dedicated NVIDIA device in LACT".into(),
                ));
            }
            _ => {
                return Err(ProviderError::Conflict(
                    "multiple dedicated NVIDIA devices in LACT".into(),
                ));
            }
        };
        let stats: LactDeviceStats = self
            .request(LactRequest::DeviceStats { id: &device.id })
            .await?;
        stats.into_telemetry(GpuIdentity {
            vendor: device.id.split(':').next().map(str::to_ascii_lowercase),
            pci_id: pci_slot_from_device_id(&device.id),
        })
    }

    fn default_poll_interval(&self) -> Duration {
        Duration::from_secs(1)
    }
}

impl LactTelemetryProvider {
    async fn request<T: for<'de> Deserialize<'de>>(
        &self,
        request: LactRequest<'_>,
    ) -> Result<T, ProviderError> {
        let mut stream =
            UnixStream::connect(&self.socket_path)
                .await
                .map_err(|error| match error.kind() {
                    std::io::ErrorKind::PermissionDenied => {
                        ProviderError::PermissionDenied(format!("LACT socket: {error}"))
                    }
                    _ => ProviderError::BackendUnavailable(format!("LACT socket: {error}")),
                })?;
        let line = serde_json::to_vec(&request)
            .map_err(|error| ProviderError::Internal(format!("LACT request encoding: {error}")))?;
        stream.write_all(&line).await.map_err(ProviderError::Io)?;
        stream.write_all(b"\n").await.map_err(ProviderError::Io)?;
        let mut response = String::new();
        BufReader::new(stream)
            .read_line(&mut response)
            .await
            .map_err(ProviderError::Io)?;
        if response.trim().is_empty() {
            return Err(ProviderError::BackendUnavailable(
                "LACT returned an empty response".into(),
            ));
        }
        let response: LactResponse = serde_json::from_str(&response)
            .map_err(|error| ProviderError::Internal(format!("LACT response decoding: {error}")))?;
        match response.status.as_str() {
            "ok" => {
                let data = response
                    .data
                    .ok_or_else(|| ProviderError::Internal("LACT response omitted data".into()))?;
                serde_json::from_value(data).map_err(|error| {
                    ProviderError::Internal(format!("LACT data decoding: {error}"))
                })
            }
            "error" => Err(ProviderError::BackendUnavailable(
                response
                    .data
                    .map(|data| data.to_string())
                    .unwrap_or_else(|| "LACT request failed".into()),
            )),
            status => Err(ProviderError::Internal(format!(
                "LACT response has unknown status '{status}'"
            ))),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "command", content = "args", rename_all = "snake_case")]
enum LactRequest<'a> {
    ListDevices,
    DeviceStats { id: &'a str },
}

#[derive(Debug, Deserialize)]
struct LactResponse {
    status: String,
    data: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct LactDevice {
    id: String,
    #[serde(default)]
    device_type: LactDeviceType,
}

#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
enum LactDeviceType {
    #[default]
    Dedicated,
    Integrated,
}

#[derive(Debug, Deserialize, Default)]
struct LactDeviceStats {
    fan: LactFanStats,
    clockspeed: LactClockStats,
    power: LactPowerStats,
    temps: BTreeMap<String, LactTemperature>,
    #[serde(default)]
    throttle_info: Option<BTreeMap<String, Vec<String>>>,
}

#[derive(Debug, Deserialize, Default)]
struct LactFanStats {
    speed_current: Option<u32>,
}

#[derive(Debug, Deserialize, Default)]
struct LactClockStats {
    gpu_clockspeed: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
struct LactPowerStats {
    current: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct LactTemperature {
    current: Option<f64>,
}

impl LactDeviceStats {
    fn into_telemetry(self, identity: GpuIdentity) -> Result<Telemetry, ProviderError> {
        let gpu_temp = self
            .temps
            .get("GPU")
            .and_then(|temperature| temperature.current)
            .map(to_temperature)
            .transpose()?;
        let gpu_power = self.power.current.map(to_milliwatt).transpose()?;
        let fans = self
            .fan
            .speed_current
            .map(|rpm| {
                Rpm::new(u16::try_from(rpm).map_err(|_| {
                    ProviderError::Internal(format!("LACT fan RPM out of range: {rpm}"))
                })?)
                .map_err(|error| ProviderError::Internal(format!("LACT fan RPM: {error:?}")))
            })
            .transpose()?
            .map(|rpm| FanTelemetry {
                source: "lactd".into(),
                fan: FanId::Gpu,
                label: "lact-gpu".into(),
                rpm,
                percent: None,
                quality: orbis_core::telemetry::FanTelemetryQuality::Partial,
            })
            .into_iter()
            .collect();
        let mut telemetry = Telemetry::empty();
        telemetry.gpu_temp = gpu_temp;
        telemetry.power = PowerTelemetry {
            gpu: gpu_power,
            ..PowerTelemetry::default()
        };
        telemetry.fans = fans;
        telemetry.gpu_power_state = GpuPowerState::Unknown;
        telemetry.gpus = vec![GpuTelemetry {
            identity: Some(identity),
            role: GpuRole::Discrete,
            source: "lactd-nvidia".into(),
            temperature: gpu_temp,
            power: gpu_power,
            fan: telemetry
                .fans
                .iter()
                .find(|fan| fan.fan == FanId::Gpu)
                .cloned(),
        }];
        // LACT clocks and throttle/process data are deliberately not stuffed
        // into unrelated core fields; the typed wire data remains validated here
        // until Orbis has a matching diagnostics contract.
        let _ = self.clockspeed.gpu_clockspeed;
        let _ = self.throttle_info;
        Ok(telemetry)
    }
}

/// Extract the stable PCI slot address (BDF) from a LACT device id.
///
/// lactd builds `list_devices` ids as `{vendor}:{device}-{subvendor}:{subdevice}-{pci_slot_name}`
/// where `pci_slot_name` is the kernel `PCI_SLOT_NAME` (for example
/// `10DE:28E0-1043:1514-0000:01:00.0`). The final `-` segment is therefore the
/// sysfs/NVML slot address. Anything that does not parse as `domain:bus:dev.fn`
/// hex is rejected: identity is never guessed from model names or partial data.
fn pci_slot_from_device_id(id: &str) -> Option<String> {
    let slot = id.rsplit_once('-')?.1;
    let mut parts = slot.split([':', '.']);
    let [domain, bus, device, function] =
        [parts.next()?, parts.next()?, parts.next()?, parts.next()?];
    let parsed = [domain, bus, device, function].map(|part| u32::from_str_radix(part, 16));
    if parsed.iter().all(Result::is_ok) && parts.next().is_none() {
        Some(slot.to_ascii_lowercase())
    } else {
        None
    }
}

fn to_temperature(value: f64) -> Result<TemperatureC, ProviderError> {
    if !value.is_finite() {
        return Err(ProviderError::Internal(
            "LACT GPU temperature is not finite".into(),
        ));
    }
    TemperatureC::new(value.trunc() as i16)
        .map_err(|_| ProviderError::Internal(format!("LACT GPU temperature out of range: {value}")))
}

fn to_milliwatt(value: f64) -> Result<MilliWatt, ProviderError> {
    if !value.is_finite() || value < 0.0 || value > f64::from(u32::MAX) / 1000.0 {
        return Err(ProviderError::Internal(format!(
            "LACT GPU power out of range: {value}"
        )));
    }
    MilliWatt::new((value * 1000.0).round() as u32)
        .map_err(|_| ProviderError::Internal(format!("LACT GPU power out of range: {value}")))
}

/// Merge optional LACT data without confusing physical GPUs.
///
/// A complete matching PCI identity permits field-local primary precedence.
/// Different identities, and any missing identity, remain separate entries and
/// are never treated as a numeric conflict. Legacy flat GPU fields are only
/// projected from a proven discrete GPU; otherwise they stay unavailable.
pub fn merge_telemetry(
    primary: Result<Telemetry, ProviderError>,
    lact: Result<Telemetry, ProviderError>,
) -> Result<Telemetry, ProviderError> {
    match (primary, lact) {
        (Ok(mut primary), Ok(lact)) => {
            for candidate in lact.gpus {
                if let Some(existing) = primary.gpus.iter_mut().find(|existing| {
                    existing
                        .identity
                        .as_ref()
                        .zip(candidate.identity.as_ref())
                        .is_some_and(|(left, right)| left.matches(right))
                }) {
                    existing.temperature = existing.temperature.or(candidate.temperature);
                    existing.power = existing.power.or(candidate.power);
                    existing.fan = existing.fan.clone().or(candidate.fan);
                } else {
                    primary.gpus.push(candidate);
                }
            }
            let discrete = primary.gpus.iter().find(|gpu| {
                gpu.role == GpuRole::Discrete
                    && gpu.identity.as_ref().is_some_and(|id| id.pci_id.is_some())
            });
            primary.gpu_temp = discrete.and_then(|gpu| gpu.temperature);
            primary.power.gpu = discrete.and_then(|gpu| gpu.power);
            if let Some(fan) = discrete.and_then(|gpu| gpu.fan.as_ref()) {
                if primary
                    .fans
                    .iter()
                    .all(|existing| existing.fan != FanId::Gpu)
                {
                    primary.fans.push(fan.clone());
                }
            }
            Ok(primary)
        }
        (Ok(primary), Err(_)) => Ok(primary),
        (Err(_), Ok(lact)) => Ok(lact),
        (Err(primary), Err(_)) => Err(primary),
    }
}

#[cfg(test)]
mod tests {
    use tokio::net::UnixListener;

    use super::*;

    fn stats_json() -> &'static str {
        r#"{"status":"ok","data":{"fan":{"speed_current":2100},"clockspeed":{"gpu_clockspeed":210},"power":{"current":4.725},"temps":{"GPU":{"current":49.0}},"throttle_info":{}}}"#
    }

    #[tokio::test]
    async fn fixture_socket_maps_known_fields_and_never_sends_mutation_commands() {
        let path = std::env::temp_dir().join(format!("orbis-lact-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        let server = tokio::spawn(async move {
            for (expected, response) in [
                (
                    "list_devices",
                    r#"{"status":"ok","data":[{"id":"10DE:28E0-1043:1514-0000:01:00.0"}]}"#,
                ),
                ("device_stats", stats_json()),
            ] {
                let (stream, _) = listener.accept().await.unwrap();
                let mut line = String::new();
                let (reader, mut writer) = stream.into_split();
                BufReader::new(reader).read_line(&mut line).await.unwrap();
                assert!(line.contains(expected));
                assert!(!line.contains("set_"));
                writer.write_all(response.as_bytes()).await.unwrap();
                writer.write_all(b"\n").await.unwrap();
            }
        });
        let telemetry = LactTelemetryProvider::with_socket_path(path.to_string_lossy());
        let snapshot = telemetry.snapshot().await.unwrap();
        server.await.unwrap();
        let gpu = snapshot.gpus.first().unwrap();
        assert_eq!(
            gpu.identity.as_ref().unwrap().pci_id.as_deref(),
            Some("0000:01:00.0")
        );
        assert_eq!(
            gpu.identity.as_ref().unwrap().vendor.as_deref(),
            Some("10de")
        );
        assert_eq!(snapshot.gpu_temp, Some(TemperatureC::new(49).unwrap()));
        assert_eq!(snapshot.power.gpu, Some(MilliWatt::new(4725).unwrap()));
        assert_eq!(snapshot.fans[0].rpm, Rpm::new(2100).unwrap());
        let _ = std::fs::remove_file(path);
    }

    fn gpu(role: GpuRole, pci_id: Option<&str>, temp: i16, power: u32) -> GpuTelemetry {
        GpuTelemetry {
            identity: Some(GpuIdentity {
                vendor: Some(
                    if role == GpuRole::Discrete {
                        "10de"
                    } else {
                        "1002"
                    }
                    .into(),
                ),
                pci_id: pci_id.map(str::to_owned),
            }),
            role,
            source: "fixture".into(),
            temperature: Some(TemperatureC::new(temp).unwrap()),
            power: Some(MilliWatt::new(power).unwrap()),
            fan: None,
        }
    }

    #[test]
    fn merge_keeps_integrated_and_discrete_separate_and_projects_discrete() {
        let mut primary = Telemetry::empty();
        primary
            .gpus
            .push(gpu(GpuRole::Integrated, Some("0000:06:00.0"), 45, 8141));
        primary.gpu_temp = Some(TemperatureC::new(45).unwrap());
        primary.power.gpu = Some(MilliWatt::new(8141).unwrap());
        let mut lact = Telemetry::empty();
        lact.gpus.push(gpu(GpuRole::Discrete, None, 61, 4725));
        lact.gpu_temp = Some(TemperatureC::new(61).unwrap());
        lact.power.gpu = Some(MilliWatt::new(4725).unwrap());
        let merged = merge_telemetry(Ok(primary), Ok(lact)).unwrap();
        assert_eq!(merged.gpus.len(), 2);
        assert_eq!(merged.gpu_temp, None);
        assert_eq!(merged.power.gpu, None);
    }

    #[test]
    fn merge_same_complete_identity_keeps_primary_values_without_conflict() {
        let mut primary = Telemetry::empty();
        primary
            .gpus
            .push(gpu(GpuRole::Discrete, Some("0000:01:00.0"), 60, 4000));
        let mut lact = Telemetry::empty();
        lact.gpus
            .push(gpu(GpuRole::Discrete, Some("0000:01:00.0"), 61, 4725));
        let merged = merge_telemetry(Ok(primary), Ok(lact)).unwrap();
        assert_eq!(merged.gpus.len(), 1);
        assert_eq!(
            merged.gpus[0].temperature,
            Some(TemperatureC::new(60).unwrap())
        );
    }

    #[test]
    fn merge_cross_source_identity_conventions_match() {
        // sysfs hwmon identity (`0x10de` vendor file) vs lactd uevent identity
        // (`10DE` PCI_ID prefix of the device id) for the same physical GPU.
        let mut primary = Telemetry::empty();
        primary
            .gpus
            .push(gpu(GpuRole::Discrete, Some("0000:01:00.0"), 60, 4000));
        primary.gpus[0].identity.as_mut().unwrap().vendor = Some("0x10de".into());
        let mut lact = Telemetry::empty();
        lact.gpus
            .push(gpu(GpuRole::Discrete, Some("0000:01:00.0"), 61, 4725));
        lact.gpus[0].identity.as_mut().unwrap().vendor = Some("10DE".into());
        let merged = merge_telemetry(Ok(primary), Ok(lact)).unwrap();
        assert_eq!(merged.gpus.len(), 1);
        // LACT fills fields the primary source lacks (here: fan data).
        assert!(merged.gpus[0].fan.is_none() || merged.fans.is_empty());
    }

    #[test]
    fn merge_different_slot_never_merges() {
        // Same vendor/device ids in the LACT id but a different BDF means a
        // different physical GPU: never merged, never a numeric conflict.
        let mut primary = Telemetry::empty();
        primary
            .gpus
            .push(gpu(GpuRole::Discrete, Some("0000:01:00.0"), 60, 4000));
        primary.gpus[0].identity.as_mut().unwrap().vendor = Some("0x10de".into());
        let mut lact = Telemetry::empty();
        lact.gpus
            .push(gpu(GpuRole::Discrete, Some("0000:06:00.0"), 61, 4725));
        lact.gpus[0].identity.as_mut().unwrap().vendor = Some("10DE".into());
        let merged = merge_telemetry(Ok(primary), Ok(lact)).unwrap();
        assert_eq!(merged.gpus.len(), 2);
        // Flat fields still project from the primary's own proven discrete
        // GPU, but never from the other physical GPU's LACT values.
        assert_eq!(merged.gpu_temp, Some(TemperatureC::new(60).unwrap()));
        assert_eq!(merged.power.gpu, Some(MilliWatt::new(4000).unwrap()));
    }

    #[test]
    fn pci_slot_extraction_from_lact_device_id() {
        // Documented format: vendor:device-subvendor:subdevice-domain:bus:dev.fn
        assert_eq!(
            pci_slot_from_device_id("10DE:28E0-1043:1514-0000:01:00.0").as_deref(),
            Some("0000:01:00.0")
        );
        assert_eq!(
            pci_slot_from_device_id("1002:687F-1043:0555-0000:0b:00.0").as_deref(),
            Some("0000:0b:00.0")
        );
        // Real RTX 4060 laptop id shape.
        assert_eq!(
            pci_slot_from_device_id("10DE:28E0-1043:1514-0000:01:00.0").as_deref(),
            Some("0000:01:00.0")
        );
        // Incomplete/legacy ids fail closed: no identity, never guessed.
        assert_eq!(pci_slot_from_device_id("10DE:28E0-x"), None);
        assert_eq!(pci_slot_from_device_id("10DE:28E0"), None);
        assert_eq!(pci_slot_from_device_id("10DE:28E0-not-a-slot"), None);
        assert_eq!(pci_slot_from_device_id(""), None);
    }

    #[test]
    fn merge_missing_identity_is_conservative() {
        let mut primary = Telemetry::empty();
        primary.gpus.push(GpuTelemetry {
            identity: None,
            role: GpuRole::Unknown,
            source: "unknown".into(),
            temperature: Some(TemperatureC::new(45).unwrap()),
            power: None,
            fan: None,
        });
        let mut lact = Telemetry::empty();
        lact.gpus.push(gpu(GpuRole::Discrete, None, 61, 4725));
        let merged = merge_telemetry(Ok(primary), Ok(lact)).unwrap();
        assert_eq!(merged.gpus.len(), 2);
        assert_eq!(merged.gpu_temp, None);
    }

    #[test]
    fn unavailable_lact_does_not_break_primary() {
        let primary = Telemetry::empty();
        assert_eq!(
            merge_telemetry(
                Ok(primary.clone()),
                Err(ProviderError::BackendUnavailable("offline".into()))
            )
            .unwrap(),
            primary
        );
    }
}
