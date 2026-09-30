//! Narrow Hardware1 client for independently gated product controls.
//!
//! The UI must never infer writability from sysfs/service presence. Hardware1
//! publishes an explicit mutation-status wire for Panel Overdrive, keyboard
//! brightness and Aura Static RGB. This adapter preserves that evidence and
//! validates mutation read-back without exposing a generic D-Bus call surface.

use std::path::PathBuf;
use std::time::Duration;

use orbis_core::action::ApplyResult;
use orbis_core::aura::AuraRgb;
use orbis_core::cpu_tuning::EnergyPreference;
use orbis_core::nvidia_tuning::{NvidiaAvailability, NvidiaField, NvidiaTuningState};
use orbis_providers::error::ProviderError;
use orbis_ui::cpu_tuning_runtime::{CpuTuningBackend, CpuTuningState};
use orbis_ui::nvidia_tuning_runtime::NvidiaTuningBackend;
use zbus::proxy::CacheProperties;

const CALL_TIMEOUT: Duration = Duration::from_secs(2);

// Hardware1 AuraMutationResult is a D-Bus structure with this exact ordered
// signature. A tuple keeps this UI adapter independent from daemon crate types
// and avoids adding a direct serde dependency to orbis-ui.
type AuraMutationWire = (u8, u8, u8, u8, u8, u8, u32);
type AuraEffectWire = (u32, String, (u8, u8, u8), (u8, u8, u8), u32);
type ApuMemoryMutationWire = (u8, u8, u8);

#[zbus::proxy(
    interface = "io.github.orbiscontrol.Hardware1",
    default_service = "io.github.orbiscontrol.Hardware",
    default_path = "/io/github/orbiscontrol/Hardware"
)]
trait HardwareProductControls {
    fn panel_mutation_status(&self) -> zbus::Result<u8>;
    fn set_panel_overdrive(&self, enabled: bool) -> zbus::Result<u8>;

    fn keyboard_backlight_mutation_status(&self) -> zbus::Result<u8>;
    fn set_keyboard_backlight(&self, level: u8) -> zbus::Result<u8>;

    fn aura_mutation_status(&self) -> zbus::Result<u8>;
    fn aura_kernel_effect_modes(&self) -> zbus::Result<Vec<u32>>;
    fn set_aura_static_rgb(&self, r: u8, g: u8, b: u8) -> zbus::Result<AuraMutationWire>;
    fn set_aura_effect(
        &self,
        mode: u32,
        speed: String,
        colour1: (u8, u8, u8),
        colour2: (u8, u8, u8),
    ) -> zbus::Result<AuraEffectWire>;
    fn set_aura_power(
        &self,
        zone: u32,
        boot: bool,
        awake: bool,
        sleep: bool,
        shutdown: bool,
    ) -> zbus::Result<(u32, bool, bool, bool, bool)>;

    fn aspm_mutation_status(&self) -> zbus::Result<u8>;
    fn aspm_disabled(&self) -> zbus::Result<bool>;
    fn set_aspm_disabled(&self, disabled: bool) -> zbus::Result<bool>;

    fn cpu_epp_mutation_status(&self) -> zbus::Result<u8>;
    fn cpu_boost_mutation_status(&self) -> zbus::Result<u8>;
    fn set_cpu_epp(&self, preference: u8) -> zbus::Result<u8>;
    fn set_cpu_boost(&self, enabled: bool) -> zbus::Result<bool>;
    fn curve_optimizer_mutation_status(&self) -> zbus::Result<u8>;
    fn set_curve_optimizer(&self, offset: i32) -> zbus::Result<i32>;
    /// Same call with ALLOW_INTERACTIVE_AUTHORIZATION: polkit may prompt.
    #[zbus(name = "SetCurveOptimizer", allow_interactive_auth)]
    fn set_curve_optimizer_interactive(&self, offset: i32) -> zbus::Result<i32>;

    fn nvidia_tuning_mutation_status(&self) -> zbus::Result<u8>;
    fn set_nvidia_tuning(&self, field: u8, value: i32) -> zbus::Result<i32>;
    /// Same call with ALLOW_INTERACTIVE_AUTHORIZATION: polkit may prompt.
    #[zbus(name = "SetNvidiaTuning", allow_interactive_auth)]
    fn set_nvidia_tuning_interactive(&self, field: u8, value: i32) -> zbus::Result<i32>;

    fn boot_sound_mutation_status(&self) -> zbus::Result<u8>;
    fn set_boot_sound(&self, enabled: bool) -> zbus::Result<u8>;

    fn apu_memory_state(&self) -> zbus::Result<u8>;
    fn apu_memory_mutation_status(&self) -> zbus::Result<u8>;
    fn set_apu_memory(&self, value: u8) -> zbus::Result<ApuMemoryMutationWire>;
}

const AURA_OUTCOME_CONFIG_CONFIRMED: u32 = 0;
const AURA_OUTCOME_KERNEL_DISPATCHED: u32 = 1;

/// Mode, speed and colours of an Aura effect as the UI shows them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuraEffectSnapshot {
    pub(crate) mode: u32,
    pub(crate) speed: String,
    pub(crate) colour1: AuraRgb,
    pub(crate) colour2: AuraRgb,
}

/// Effect the kernel `kbd_rgb_mode` route last accepted. asusd cannot see that
/// attribute, so its config keeps reporting the old effect; the memo is shown
/// only while asusd's config is still what it was right after the write.
struct KernelEffectSlot(std::sync::Mutex<Option<KernelEffectMemo>>);

struct KernelEffectMemo {
    sent: AuraEffectSnapshot,
    asusd_baseline: Option<AuraEffectSnapshot>,
}

impl KernelEffectSlot {
    const fn new() -> Self {
        Self(std::sync::Mutex::new(None))
    }

    fn remember(&self, sent: AuraEffectSnapshot) {
        *self.0.lock().unwrap() = Some(KernelEffectMemo {
            sent,
            asusd_baseline: None,
        });
    }

    fn forget(&self) {
        *self.0.lock().unwrap() = None;
    }

    fn overlay(&self, asusd_now: &AuraEffectSnapshot) -> Option<AuraEffectSnapshot> {
        let mut slot = self.0.lock().unwrap();
        let memo = slot.as_mut()?;
        match &memo.asusd_baseline {
            None => memo.asusd_baseline = Some(asusd_now.clone()),
            Some(baseline) if baseline != asusd_now => {
                *slot = None;
                return None;
            }
            Some(_) => {}
        }
        slot.as_ref().map(|memo| memo.sent.clone())
    }
}

static KERNEL_EFFECT: KernelEffectSlot = KernelEffectSlot::new();

pub(crate) fn forget_kernel_effect() {
    KERNEL_EFFECT.forget();
}

/// The effect to show instead of asusd's when the kernel route owns the
/// keyboard, or `None` when asusd's own state is authoritative.
pub(crate) fn kernel_effect_overlay(asusd_now: &AuraEffectSnapshot) -> Option<AuraEffectSnapshot> {
    KERNEL_EFFECT.overlay(asusd_now)
}

/// `true` when asusd's config confirmed the effect, `false` when it was only
/// dispatched to the write-only kernel attribute.
fn decode_aura_effect_outcome(outcome: u32) -> Result<bool, ProviderError> {
    match outcome {
        AURA_OUTCOME_CONFIG_CONFIRMED => Ok(true),
        AURA_OUTCOME_KERNEL_DISPATCHED => Ok(false),
        other => Err(ProviderError::Internal(format!(
            "Hardware1 Aura effect returned unknown outcome {other}"
        ))),
    }
}

/// Effective write evidence published by Hardware1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProductWriteStatus {
    Supported,
    Unsupported,
    TemporarilyUnavailable,
    PermissionDenied,
    Conflicted,
    Unknown,
}

impl ProductWriteStatus {
    pub(crate) fn is_supported(self) -> bool {
        matches!(self, Self::Supported)
    }

    pub(crate) fn short_label(self) -> &'static str {
        match self {
            Self::Supported => "write ready",
            Self::Unsupported => "write disabled",
            Self::TemporarilyUnavailable => "write unavailable",
            Self::PermissionDenied => "write denied",
            Self::Conflicted => "write conflicted",
            Self::Unknown => "write unknown",
        }
    }
}

/// Config-confirmed Aura observation. Hardware state is not readable, so a
/// validated Hardware1 reply remains `Accepted`, never `Applied`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuraConfigObservation {
    pub(crate) requested: AuraRgb,
    pub(crate) observed: AuraRgb,
    pub(crate) result: ApplyResult,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuraEffectObservation {
    pub(crate) mode: u32,
    pub(crate) speed: String,
    pub(crate) colour1: AuraRgb,
    pub(crate) colour2: AuraRgb,
    pub(crate) result: ApplyResult,
    /// True when asusd's config read-back matched; false when the effect was
    /// only dispatched to the write-only kernel attribute.
    pub(crate) confirmed: bool,
}

/// Narrow client over one externally-owned system-bus connection.
#[derive(Clone)]
pub(crate) struct HardwareProductControlClient {
    connection: zbus::Connection,
}

impl HardwareProductControlClient {
    pub(crate) fn new(connection: zbus::Connection) -> Self {
        Self { connection }
    }

    pub(crate) async fn connect_system() -> Result<Self, ProviderError> {
        let connection = tokio::time::timeout(CALL_TIMEOUT, zbus::Connection::system())
            .await
            .map_err(|_| ProviderError::Timeout("Hardware1 system-bus connect timed out".into()))?
            .map_err(zbus_error_to_provider)?;
        Ok(Self::new(connection))
    }

    async fn proxy(&self) -> Result<HardwareProductControlsProxy<'_>, ProviderError> {
        HardwareProductControlsProxy::builder(&self.connection)
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .map_err(zbus_error_to_provider)
    }

    pub(crate) async fn panel_status(&self) -> Result<ProductWriteStatus, ProviderError> {
        let proxy = self.proxy().await?;
        let raw = timed("panel mutation status", proxy.panel_mutation_status()).await?;
        decode_status(raw, "panel")
    }

    pub(crate) async fn keyboard_status(&self) -> Result<ProductWriteStatus, ProviderError> {
        let proxy = self.proxy().await?;
        let raw = timed(
            "keyboard mutation status",
            proxy.keyboard_backlight_mutation_status(),
        )
        .await?;
        decode_status(raw, "keyboard")
    }

    pub(crate) async fn aura_status(&self) -> Result<ProductWriteStatus, ProviderError> {
        let proxy = self.proxy().await?;
        let raw = timed("Aura mutation status", proxy.aura_mutation_status()).await?;
        decode_status(raw, "Aura")
    }

    /// Effect modes the kernel `kbd_rgb_mode` route can apply beyond asusd's list.
    pub(crate) async fn aura_kernel_effect_modes(&self) -> Result<Vec<u32>, ProviderError> {
        let proxy = self.proxy().await?;
        timed("Aura kernel effect modes", proxy.aura_kernel_effect_modes()).await
    }

    pub(crate) async fn boot_sound_status(&self) -> Result<ProductWriteStatus, ProviderError> {
        let proxy = self.proxy().await?;
        let raw = timed(
            "boot sound mutation status",
            proxy.boot_sound_mutation_status(),
        )
        .await?;
        decode_status(raw, "boot sound")
    }

    pub(crate) async fn aspm_state(&self) -> Result<(bool, ProductWriteStatus), ProviderError> {
        let proxy = self.proxy().await?;
        let status = decode_status(
            timed("ASPM mutation status", proxy.aspm_mutation_status()).await?,
            "ASPM",
        )?;
        let disabled = timed("ASPM state", proxy.aspm_disabled()).await?;
        Ok((disabled, status))
    }

    pub(crate) async fn cpu_epp_status(&self) -> Result<ProductWriteStatus, ProviderError> {
        let proxy = self.proxy().await?;
        decode_status(
            timed("CPU EPP mutation status", proxy.cpu_epp_mutation_status()).await?,
            "CPU EPP",
        )
    }

    pub(crate) async fn cpu_boost_status(&self) -> Result<ProductWriteStatus, ProviderError> {
        let proxy = self.proxy().await?;
        decode_status(
            timed(
                "CPU boost mutation status",
                proxy.cpu_boost_mutation_status(),
            )
            .await?,
            "CPU boost",
        )
    }

    pub(crate) async fn curve_optimizer_status(&self) -> Result<ProductWriteStatus, ProviderError> {
        let proxy = self.proxy().await?;
        decode_status(
            timed(
                "Curve Optimizer mutation status",
                proxy.curve_optimizer_mutation_status(),
            )
            .await?,
            "Curve Optimizer",
        )
    }

    pub(crate) async fn set_curve_optimizer(
        &self,
        offset: i32,
        interactive: bool,
    ) -> Result<(), ProviderError> {
        require_supported(self.curve_optimizer_status().await?, "Curve Optimizer")?;
        let proxy = self.proxy().await?;
        let confirmed = if interactive {
            // A password prompt can take a while; no mutation timeout here.
            proxy
                .set_curve_optimizer_interactive(offset)
                .await
                .map_err(zbus_error_to_provider)?
        } else {
            timed(
                "Curve Optimizer mutation",
                proxy.set_curve_optimizer(offset),
            )
            .await?
        };
        if confirmed != offset {
            return Err(ProviderError::BackendUnavailable(format!(
                "Hardware1 Curve Optimizer mismatch: requested={offset}, returned={confirmed}"
            )));
        }
        Ok(())
    }

    pub(crate) async fn set_cpu_epp(
        &self,
        preference: EnergyPreference,
    ) -> Result<(), ProviderError> {
        require_supported(self.cpu_epp_status().await?, "CPU EPP")?;
        let proxy = self.proxy().await?;
        let confirmed = timed("CPU EPP mutation", proxy.set_cpu_epp(preference.wire())).await?;
        if confirmed != preference.wire() {
            return Err(ProviderError::BackendUnavailable(format!(
                "Hardware1 CPU EPP read-back mismatch: requested={}, returned={confirmed}",
                preference.wire()
            )));
        }
        Ok(())
    }

    pub(crate) async fn set_cpu_boost(&self, enabled: bool) -> Result<(), ProviderError> {
        require_supported(self.cpu_boost_status().await?, "CPU boost")?;
        let proxy = self.proxy().await?;
        let confirmed = timed("CPU boost mutation", proxy.set_cpu_boost(enabled)).await?;
        if confirmed != enabled {
            return Err(ProviderError::BackendUnavailable(format!(
                "Hardware1 CPU boost read-back mismatch: requested={enabled}, returned={confirmed}"
            )));
        }
        Ok(())
    }

    pub(crate) async fn nvidia_tuning_status(&self) -> Result<ProductWriteStatus, ProviderError> {
        let proxy = self.proxy().await?;
        decode_status(
            timed(
                "NVIDIA tuning mutation status",
                proxy.nvidia_tuning_mutation_status(),
            )
            .await?,
            "NVIDIA tuning",
        )
    }

    pub(crate) async fn set_nvidia_tuning(
        &self,
        field: NvidiaField,
        value: i32,
        interactive: bool,
    ) -> Result<(), ProviderError> {
        require_supported(self.nvidia_tuning_status().await?, "NVIDIA tuning")?;
        let proxy = self.proxy().await?;
        let confirmed = if interactive {
            // A password prompt can take a while; no mutation timeout here.
            proxy
                .set_nvidia_tuning_interactive(field.wire(), value)
                .await
                .map_err(zbus_error_to_provider)?
        } else {
            timed(
                "NVIDIA tuning mutation",
                proxy.set_nvidia_tuning(field.wire(), value),
            )
            .await?
        };
        if confirmed != value {
            return Err(ProviderError::BackendUnavailable(format!(
                "Hardware1 NVIDIA {} read-back mismatch: requested={value}, returned={confirmed}",
                field.label()
            )));
        }
        Ok(())
    }

    pub(crate) async fn set_aspm_disabled(&self, disabled: bool) -> Result<bool, ProviderError> {
        let proxy = self.proxy().await?;
        timed("ASPM mutation", proxy.set_aspm_disabled(disabled)).await
    }

    pub(crate) async fn set_boot_sound(&self, enabled: bool) -> Result<bool, ProviderError> {
        require_supported(self.boot_sound_status().await?, "boot sound")?;
        let proxy = self.proxy().await?;
        let raw = timed("boot sound mutation", proxy.set_boot_sound(enabled)).await?;
        let observed = match raw {
            0 => false,
            1 => true,
            other => {
                return Err(ProviderError::Internal(format!(
                    "Hardware1 boot sound returned non-boolean read-back {other}"
                )));
            }
        };
        if observed != enabled {
            return Err(ProviderError::BackendUnavailable(format!(
                "Hardware1 boot sound read-back mismatch: requested={enabled}, observed={observed}"
            )));
        }
        Ok(observed)
    }

    pub(crate) async fn apu_memory_state(&self) -> Result<u8, ProviderError> {
        let proxy = self.proxy().await?;
        let value = timed("iGPU memory state", proxy.apu_memory_state()).await?;
        if value > 8 {
            return Err(ProviderError::Internal(format!(
                "Hardware1 apu_mem returned out-of-range value {value}"
            )));
        }
        Ok(value)
    }

    pub(crate) async fn apu_memory_status(&self) -> Result<ProductWriteStatus, ProviderError> {
        let proxy = self.proxy().await?;
        decode_status(
            timed(
                "iGPU memory mutation status",
                proxy.apu_memory_mutation_status(),
            )
            .await?,
            "iGPU memory",
        )
    }

    pub(crate) async fn set_apu_memory(&self, value: u8) -> Result<(u8, bool), ProviderError> {
        if value > 8 {
            return Err(ProviderError::InvalidRequest(format!(
                "iGPU memory value must be 0..=8, got {value}"
            )));
        }
        require_supported(self.apu_memory_status().await?, "iGPU memory")?;
        let proxy = self.proxy().await?;
        let wire = timed("iGPU memory mutation", proxy.set_apu_memory(value)).await?;
        if wire.0 != value || wire.1 != value {
            return Err(ProviderError::BackendUnavailable(format!(
                "Hardware1 iGPU memory read-back mismatch: requested={value}, returned=({}, {})",
                wire.0, wire.1
            )));
        }
        if wire.2 != 1 {
            return Err(ProviderError::Internal(format!(
                "Hardware1 iGPU memory returned unknown outcome {}",
                wire.2
            )));
        }
        Ok((wire.1, true))
    }

    pub(crate) async fn set_panel_overdrive(&self, enabled: bool) -> Result<bool, ProviderError> {
        require_supported(self.panel_status().await?, "Panel Overdrive")?;
        let proxy = self.proxy().await?;
        let raw = timed(
            "Panel Overdrive mutation",
            proxy.set_panel_overdrive(enabled),
        )
        .await?;
        let observed = match raw {
            0 => false,
            1 => true,
            other => {
                return Err(ProviderError::Internal(format!(
                    "Hardware1 Panel Overdrive returned non-boolean read-back {other}"
                )));
            }
        };
        if observed != enabled {
            return Err(ProviderError::BackendUnavailable(format!(
                "Hardware1 Panel Overdrive read-back mismatch: requested={enabled}, observed={observed}"
            )));
        }
        Ok(observed)
    }

    pub(crate) async fn set_keyboard_backlight(&self, level: u8) -> Result<u8, ProviderError> {
        require_supported(self.keyboard_status().await?, "Keyboard Backlight")?;
        let proxy = self.proxy().await?;
        let observed = timed(
            "keyboard backlight mutation",
            proxy.set_keyboard_backlight(level),
        )
        .await?;
        if observed != level {
            return Err(ProviderError::BackendUnavailable(format!(
                "Hardware1 keyboard read-back mismatch: requested={level}, observed={observed}"
            )));
        }
        Ok(observed)
    }

    #[allow(dead_code)]
    pub(crate) async fn set_aura_static_rgb(
        &self,
        requested: AuraRgb,
    ) -> Result<AuraConfigObservation, ProviderError> {
        require_supported(self.aura_status().await?, "Aura Static RGB")?;
        let proxy = self.proxy().await?;
        let wire = timed(
            "Aura Static RGB mutation",
            proxy.set_aura_static_rgb(requested.r, requested.g, requested.b),
        )
        .await?;

        let returned_requested = AuraRgb {
            r: wire.0,
            g: wire.1,
            b: wire.2,
        };
        let observed = AuraRgb {
            r: wire.3,
            g: wire.4,
            b: wire.5,
        };
        if returned_requested != requested || observed != requested {
            return Err(ProviderError::BackendUnavailable(format!(
                "Hardware1 Aura config read-back mismatch: requested={requested:?}, returned={returned_requested:?}, observed={observed:?}"
            )));
        }
        if wire.6 != AURA_OUTCOME_CONFIG_CONFIRMED {
            return Err(ProviderError::Internal(format!(
                "Hardware1 Aura returned unknown outcome {}",
                wire.6
            )));
        }

        forget_kernel_effect();
        Ok(AuraConfigObservation {
            requested,
            observed,
            result: ApplyResult::Accepted,
        })
    }

    /// Set one zone's boot/awake/sleep/shutdown lighting; the daemon confirms
    /// it by reading asusd back, and this checks the echo once more.
    pub(crate) async fn set_aura_power(
        &self,
        requested: orbis_core::aura::AuraPowerState,
    ) -> Result<orbis_core::aura::AuraPowerState, ProviderError> {
        require_supported(self.aura_status().await?, "Aura power states")?;
        let proxy = self.proxy().await?;
        let (zone, boot, awake, sleep, shutdown) = timed(
            "Aura power-state mutation",
            proxy.set_aura_power(
                requested.zone,
                requested.boot,
                requested.awake,
                requested.sleep,
                requested.shutdown,
            ),
        )
        .await?;
        let observed = orbis_core::aura::AuraPowerState {
            zone,
            boot,
            awake,
            sleep,
            shutdown,
        };
        if observed != requested {
            return Err(ProviderError::BackendUnavailable(
                "Hardware1 Aura power-state read-back mismatch".into(),
            ));
        }
        Ok(observed)
    }

    pub(crate) async fn set_aura_effect(
        &self,
        mode: u32,
        speed: String,
        colour1: AuraRgb,
        colour2: AuraRgb,
    ) -> Result<AuraEffectObservation, ProviderError> {
        require_supported(self.aura_status().await?, "Aura effect")?;
        let proxy = self.proxy().await?;
        let wire = timed(
            "Aura effect mutation",
            proxy.set_aura_effect(
                mode,
                speed.clone(),
                (colour1.r, colour1.g, colour1.b),
                (colour2.r, colour2.g, colour2.b),
            ),
        )
        .await?;
        let observed_colour1 = AuraRgb {
            r: wire.2.0,
            g: wire.2.1,
            b: wire.2.2,
        };
        let observed_colour2 = AuraRgb {
            r: wire.3.0,
            g: wire.3.1,
            b: wire.3.2,
        };
        if wire.0 != mode
            || wire.1 != speed
            || observed_colour1 != colour1
            || observed_colour2 != colour2
        {
            return Err(ProviderError::BackendUnavailable(
                "Hardware1 Aura effect config read-back mismatch".into(),
            ));
        }
        let confirmed = decode_aura_effect_outcome(wire.4)?;
        let observation = AuraEffectObservation {
            mode: wire.0,
            speed: wire.1,
            colour1: observed_colour1,
            colour2: observed_colour2,
            result: ApplyResult::Accepted,
            confirmed,
        };
        if confirmed {
            KERNEL_EFFECT.forget();
        } else {
            KERNEL_EFFECT.remember(AuraEffectSnapshot {
                mode: observation.mode,
                speed: observation.speed.clone(),
                colour1: observation.colour1,
                colour2: observation.colour2,
            });
        }
        Ok(observation)
    }
}

async fn timed<T>(
    label: &'static str,
    future: impl std::future::Future<Output = zbus::Result<T>>,
) -> Result<T, ProviderError> {
    tokio::time::timeout(CALL_TIMEOUT, future)
        .await
        .map_err(|_| ProviderError::Timeout(format!("{label} timed out")))?
        .map_err(zbus_error_to_provider)
}

fn decode_status(raw: u8, feature: &str) -> Result<ProductWriteStatus, ProviderError> {
    match raw {
        0 => Ok(ProductWriteStatus::Supported),
        1 => Ok(ProductWriteStatus::Unsupported),
        2 => Ok(ProductWriteStatus::TemporarilyUnavailable),
        3 => Ok(ProductWriteStatus::PermissionDenied),
        4 => Ok(ProductWriteStatus::Unknown),
        5 => Ok(ProductWriteStatus::Conflicted),
        other => Err(ProviderError::Internal(format!(
            "Hardware1 {feature} mutation status returned unknown wire value {other}"
        ))),
    }
}

pub(crate) fn require_supported(
    status: ProductWriteStatus,
    feature: &str,
) -> Result<(), ProviderError> {
    match status {
        ProductWriteStatus::Supported => Ok(()),
        ProductWriteStatus::Unsupported => Err(ProviderError::Unsupported(format!(
            "{feature} mutation is product/backend disabled"
        ))),
        ProductWriteStatus::TemporarilyUnavailable => Err(ProviderError::BackendUnavailable(
            format!("{feature} mutation is temporarily unavailable"),
        )),
        ProductWriteStatus::PermissionDenied => Err(ProviderError::PermissionDenied(format!(
            "{feature} mutation permission denied"
        ))),
        ProductWriteStatus::Conflicted => Err(ProviderError::Conflict(format!(
            "{feature} mutation conflicts with another owner"
        ))),
        ProductWriteStatus::Unknown => Err(ProviderError::BackendUnavailable(format!(
            "{feature} mutation readiness is unknown"
        ))),
    }
}

fn zbus_error_to_provider(error: zbus::Error) -> ProviderError {
    if let zbus::Error::FDO(boxed) = &error {
        return match &**boxed {
            zbus::fdo::Error::NotSupported(message) => ProviderError::Unsupported(message.clone()),
            zbus::fdo::Error::AccessDenied(message) => {
                ProviderError::PermissionDenied(message.clone())
            }
            zbus::fdo::Error::InvalidArgs(message) => {
                ProviderError::InvalidRequest(message.clone())
            }
            _ => ProviderError::Dbus(error.to_string()),
        };
    }
    ProviderError::Dbus(error.to_string())
}

const CPU_SYSFS_ROOT: &str = "/sys/devices/system/cpu";

/// Production CPU tuning backend: sysfs reads, typed Hardware1 writes.
pub(crate) struct SystemCpuTuning {
    client: HardwareProductControlClient,
    root: PathBuf,
    curve_optimizer_applied: std::sync::Mutex<Option<i32>>,
}

impl SystemCpuTuning {
    pub(crate) fn new(connection: zbus::Connection) -> Self {
        Self {
            client: HardwareProductControlClient::new(connection),
            root: PathBuf::from(CPU_SYSFS_ROOT),
            curve_optimizer_applied: Default::default(),
        }
    }
}

#[async_trait::async_trait]
impl CpuTuningBackend for SystemCpuTuning {
    async fn read(&self) -> CpuTuningState {
        let epp_raw =
            std::fs::read_to_string(self.root.join("cpu0/cpufreq/energy_performance_preference"))
                .ok();
        let boost_raw = std::fs::read_to_string(self.root.join("cpufreq/boost")).ok();
        let epp_supported = epp_raw.is_some();
        let boost = boost_raw.as_deref().and_then(|raw| match raw.trim() {
            "1" => Some(true),
            "0" => Some(false),
            _ => None,
        });
        let curve_optimizer_applied = *self.curve_optimizer_applied.lock().unwrap();
        let installed = orbis_providers::amd_tuning::is_installed(
            std::path::Path::new(orbis_providers::amd_tuning::RYZENADJ_EXECUTABLE),
            std::path::Path::new(orbis_providers::amd_tuning::SMU_SYSFS_ROOT),
        );
        let co_status = if installed {
            Some(self.client.curve_optimizer_status().await)
        } else {
            None
        };
        CpuTuningState {
            curve_optimizer_supported: co_status
                .as_ref()
                .is_some_and(|status| !matches!(status, Ok(ProductWriteStatus::Unsupported))),
            curve_optimizer_applied,
            curve_optimizer_writable: co_status
                .as_ref()
                .is_some_and(|status| status.as_ref().is_ok_and(|s| s.is_supported())),
            epp: epp_raw.as_deref().and_then(EnergyPreference::from_sysfs),
            epp_supported,
            epp_writable: epp_supported
                && self
                    .client
                    .cpu_epp_status()
                    .await
                    .is_ok_and(|status| status.is_supported()),
            boost,
            boost_writable: boost.is_some()
                && self
                    .client
                    .cpu_boost_status()
                    .await
                    .is_ok_and(|status| status.is_supported()),
        }
    }

    async fn set_epp(&self, preference: EnergyPreference) -> Result<(), ProviderError> {
        self.client.set_cpu_epp(preference).await
    }

    async fn set_boost(&self, enabled: bool) -> Result<(), ProviderError> {
        self.client.set_cpu_boost(enabled).await
    }

    async fn set_curve_optimizer(
        &self,
        offset: i32,
        interactive: bool,
    ) -> Result<(), ProviderError> {
        self.client.set_curve_optimizer(offset, interactive).await?;
        *self.curve_optimizer_applied.lock().unwrap() = Some(offset);
        Ok(())
    }
}

/// Production NVIDIA tuning backend: NVML reads (never on a sleeping GPU),
/// typed Hardware1 writes.
pub(crate) struct SystemNvidiaTuning {
    client: HardwareProductControlClient,
}

impl SystemNvidiaTuning {
    pub(crate) fn new(connection: zbus::Connection) -> Self {
        Self {
            client: HardwareProductControlClient::new(connection),
        }
    }
}

#[async_trait::async_trait]
impl NvidiaTuningBackend for SystemNvidiaTuning {
    async fn read(&self) -> NvidiaTuningState {
        let root = std::path::Path::new(orbis_providers::nvidia_tuning::SYSFS_ROOT);
        let mut state = tokio::task::spawn_blocking(|| {
            orbis_providers::nvidia_tuning::read_state(
                std::path::Path::new(orbis_providers::nvidia_tuning::SYSFS_ROOT),
                &orbis_providers::nvidia_tuning::NvmlDriver,
            )
        })
        .await
        .unwrap_or_else(|_| NvidiaTuningState {
            availability: orbis_providers::nvidia_tuning::availability(root),
            ..NvidiaTuningState::default()
        });
        state.writable = state.has_settings()
            && self
                .client
                .nvidia_tuning_status()
                .await
                .is_ok_and(|status| status.is_supported());
        state
    }

    async fn availability(&self) -> NvidiaAvailability {
        orbis_providers::nvidia_tuning::availability(std::path::Path::new(
            orbis_providers::nvidia_tuning::SYSFS_ROOT,
        ))
    }

    async fn set(
        &self,
        field: NvidiaField,
        value: i32,
        interactive: bool,
    ) -> Result<(), ProviderError> {
        self.client
            .set_nvidia_tuning(field, value, interactive)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_status_wire_is_strict() {
        assert_eq!(
            decode_status(0, "test").unwrap(),
            ProductWriteStatus::Supported
        );
        assert_eq!(
            decode_status(1, "test").unwrap(),
            ProductWriteStatus::Unsupported
        );
        assert_eq!(
            decode_status(4, "test").unwrap(),
            ProductWriteStatus::Unknown
        );
        assert_eq!(
            decode_status(5, "test").unwrap(),
            ProductWriteStatus::Conflicted
        );
        assert!(decode_status(6, "test").is_err());
    }

    #[test]
    fn only_supported_status_passes_mutation_gate() {
        assert!(require_supported(ProductWriteStatus::Supported, "test").is_ok());
        assert!(require_supported(ProductWriteStatus::Unsupported, "test").is_err());
        assert!(require_supported(ProductWriteStatus::PermissionDenied, "test").is_err());
        assert!(require_supported(ProductWriteStatus::Conflicted, "test").is_err());
        assert!(require_supported(ProductWriteStatus::Unknown, "test").is_err());
    }

    fn snapshot(mode: u32, r: u8) -> AuraEffectSnapshot {
        AuraEffectSnapshot {
            mode,
            speed: "Med".into(),
            colour1: AuraRgb { r, g: 0, b: 0 },
            colour2: AuraRgb { r: 0, g: 0, b: 0 },
        }
    }

    #[test]
    fn kernel_effect_overlay_lasts_only_while_asusd_config_is_unchanged() {
        let slot = KernelEffectSlot::new();
        let asusd = snapshot(0, 10);
        assert_eq!(slot.overlay(&asusd), None);

        slot.remember(snapshot(1, 200));
        assert_eq!(slot.overlay(&asusd), Some(snapshot(1, 200)));
        assert_eq!(slot.overlay(&asusd), Some(snapshot(1, 200)));

        assert_eq!(slot.overlay(&snapshot(0, 99)), None);
        assert_eq!(slot.overlay(&asusd), None);

        slot.remember(snapshot(3, 5));
        assert!(slot.overlay(&asusd).is_some());
        slot.forget();
        assert_eq!(slot.overlay(&asusd), None);
    }

    #[test]
    fn aura_effect_outcome_separates_confirmed_from_dispatched() {
        assert!(decode_aura_effect_outcome(0).unwrap());
        assert!(!decode_aura_effect_outcome(1).unwrap());
        assert!(decode_aura_effect_outcome(2).is_err());
    }

    #[test]
    fn aura_semantics_remain_config_confirmed_only() {
        assert!(!ApplyResult::Accepted.is_applied());
    }

    #[test]
    fn client_has_no_generic_process_filesystem_gpu_or_fan_surface() {
        let source = include_str!("hardware_controls_backend.rs");
        let forbidden = [
            ["std::fs::", "write"].concat(),
            ["Command", "::new"].concat(),
            ["set_", "gpu_mode"].concat(),
            ["set_", "fan_curve"].concat(),
        ];
        for token in forbidden {
            assert!(
                !source.contains(&token),
                "unexpected control surface: {token}"
            );
        }
    }
}
