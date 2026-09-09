//! Aura Static RGB mutation backend.
//!
//! Authority: asusd service `xyz.ljones.Asusd`, interface `xyz.ljones.Aura`,
//! object path `/xyz/ljones/aura/tuf` (TUF laptop keyboard).
//!
//! RGB hardware read-back невозможен: kernel `kbd_rgb_mode` — write-only
//! (`DEVICE_ATTR_WO`), поэтому authoritative подтверждение hardware state
//! отсутствует. Mutation подтверждается config-level read-back через asusd
//! property `LedModeData` (in-memory config asusd) и честно возвращает
//! [`ApplyResult::Accepted`], а не `Applied`.
//!
//! Mutation path (ровно один setter):
//! 1. fresh read `LedModeData`;
//! 2. fresh read `SupportedBasicModes`, убедиться что Static (0) поддерживается;
//! 3. сохранить неизменными: zone, colour2, speed, direction;
//! 4. заменить только primary RGB (mode фиксирован на Static — это Static RGB
//!    mutation);
//! 5. ровно один `Set LedModeData`;
//! 6. fresh read `LedModeData`;
//! 7. подтвердить config-level совпадение RGB;
//! 8. вернуть честный `Accepted` (hardware state не подтверждён).
//!
//! Никакого generic D-Bus/sysfs writer API. Никакого optimistic success.

use async_trait::async_trait;
use orbis_core::action::ApplyResult;
use orbis_core::aura::{AuraEffect, AuraMode, AuraRgb, AuraZone};
use orbis_providers::error::ProviderError;
use zbus::Connection;

use crate::{AuthorizeError, Authorizer, provider_error_to_dbus};

/// asusd D-Bus bus name.
pub const ASUSD_AURA_DESTINATION: &str = "xyz.ljones.Asusd";
/// asusd D-Bus Aura object path (TUF laptop keyboard).
pub const ASUSD_AURA_PATH: &str = "/xyz/ljones/aura/tuf";
/// asusd D-Bus Aura interface.
pub const ASUSD_AURA_INTERFACE: &str = "xyz.ljones.Aura";

/// Wire type of `LedModeData`: struct `(uu(yyy)(yyy)ss)`.
type AuraEffectWire = (u32, u32, (u8, u8, u8), (u8, u8, u8), String, String);

/// Encode a domain effect into the wire tuple (lossless for `Unknown`).
fn effect_to_wire(effect: &AuraEffect) -> AuraEffectWire {
    (
        effect.mode.to_u32(),
        effect.zone.to_u32(),
        (effect.colour1.r, effect.colour1.g, effect.colour1.b),
        (effect.colour2.r, effect.colour2.g, effect.colour2.b),
        effect.speed.as_str().to_string(),
        effect.direction.as_str().to_string(),
    )
}

/// Decode a wire tuple into a domain effect (total; unknown values preserved).
fn effect_from_wire(wire: AuraEffectWire) -> AuraEffect {
    AuraEffect {
        mode: AuraMode::from_u32(wire.0),
        zone: AuraZone::from_u32(wire.1),
        colour1: AuraRgb {
            r: wire.2.0,
            g: wire.2.1,
            b: wire.2.2,
        },
        colour2: AuraRgb {
            r: wire.3.0,
            g: wire.3.1,
            b: wire.3.2,
        },
        speed: wire.4.parse().unwrap(),
        direction: wire.5.parse().unwrap(),
    }
}

/// Typed asusd operations required by the Static RGB mutation algorithm.
#[async_trait]
pub trait AsusdAuraClient: Send + Sync {
    /// Fresh read `LedModeData` (struct `(uu(yyy)(yyy)ss)`).
    async fn led_mode_data(&self) -> Result<AuraEffect, ProviderError>;
    /// Fresh read `SupportedBasicModes` (array of u32).
    async fn supported_basic_modes(&self) -> Result<Vec<u32>, ProviderError>;
    /// Set `LedModeData` (exactly one mutation).
    async fn set_led_mode_data(&self, effect: AuraEffect) -> Result<(), ProviderError>;
}

#[zbus::proxy(
    interface = "xyz.ljones.Aura",
    default_service = "xyz.ljones.Asusd",
    default_path = "/xyz/ljones/aura/tuf"
)]
trait AsusdAura {
    #[zbus(property)]
    fn led_mode_data(&self) -> zbus::Result<AuraEffectWire>;

    #[zbus(property)]
    fn set_led_mode_data(&self, effect: AuraEffectWire) -> zbus::Result<()>;

    #[zbus(property)]
    fn supported_basic_modes(&self) -> zbus::Result<Vec<u32>>;
}

/// Production typed client for the asusd Aura backend.
pub struct ZbusAsusdAuraClient {
    connection: Connection,
}

impl ZbusAsusdAuraClient {
    /// Construct without performing a D-Bus call.
    pub fn new(connection: Connection) -> Self {
        Self { connection }
    }

    async fn proxy(&self) -> Result<AsusdAuraProxy<'_>, ProviderError> {
        AsusdAuraProxy::new(&self.connection)
            .await
            .map_err(|error| ProviderError::Dbus(format!("asusd aura proxy: {error}")))
    }
}

#[async_trait]
impl AsusdAuraClient for ZbusAsusdAuraClient {
    async fn led_mode_data(&self) -> Result<AuraEffect, ProviderError> {
        let wire = self.proxy().await?.led_mode_data().await.map_err(|error| {
            ProviderError::Dbus(format!("asusd aura led_mode_data read: {error}"))
        })?;
        Ok(effect_from_wire(wire))
    }

    async fn supported_basic_modes(&self) -> Result<Vec<u32>, ProviderError> {
        self.proxy()
            .await?
            .supported_basic_modes()
            .await
            .map_err(|error| {
                ProviderError::Dbus(format!("asusd aura supported_basic_modes read: {error}"))
            })
    }

    async fn set_led_mode_data(&self, effect: AuraEffect) -> Result<(), ProviderError> {
        self.proxy()
            .await?
            .set_led_mode_data(effect_to_wire(&effect))
            .await
            .map_err(|error| {
                ProviderError::Dbus(format!("asusd aura led_mode_data setter: {error}"))
            })
    }
}

/// Fresh result of an Aura Static RGB mutation and its config-level read-back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuraStaticRgbMutationReadback {
    /// Requested primary RGB.
    pub requested: AuraRgb,
    /// Observed primary RGB (config-level read-back via asusd `LedModeData`).
    pub observed: AuraRgb,
    /// `Accepted` — asusd принял и config-level read-back совпал; hardware
    /// state не подтверждён (`kbd_rgb_mode` write-only).
    pub result: ApplyResult,
}

/// Fresh result of a full Aura effect mutation and config-level read-back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuraEffectMutationReadback {
    pub requested: AuraEffect,
    pub observed: AuraEffect,
    pub result: ApplyResult,
}

/// Typed runtime evidence for Aura Static RGB mutation backend availability.
///
/// Preserves the distinction between a proven backend (`Supported`), a
/// structurally absent mutation capability (`Unsupported`), a temporary
/// discovery failure (`TemporarilyUnavailable`), an authorization failure
/// (`PermissionDenied`) and missing evidence (`Unknown`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuraMutationStatus {
    /// A proven production mutation backend (typed asusd client) is installed.
    Supported,
    /// Mutation capability is structurally absent.
    Unsupported,
    /// A known/expected backend is temporarily unavailable.
    TemporarilyUnavailable,
    /// Mutation exists but current authorization evidence denies it.
    PermissionDenied,
    /// No evidence about mutation availability.
    Unknown,
}

/// Stable wire values for `Hardware1.AuraMutationStatus`.
///
/// The numeric values match the Battery/Performance mutation status wire
/// contract (identical semantic classes); each backend module keeps its own
/// named constants so the D-Bus contract stays self-contained.
pub mod aura_mutation_wire {
    use super::AuraMutationStatus;

    /// Proven mutation backend / ABI present.
    pub const SUPPORTED: u8 = 0;
    /// Mutation capability structurally absent.
    pub const UNSUPPORTED: u8 = 1;
    /// Known backend temporarily unavailable.
    pub const TEMPORARILY_UNAVAILABLE: u8 = 2;
    /// Mutation denied by authorization evidence.
    pub const PERMISSION_DENIED: u8 = 3;
    /// No evidence.
    pub const UNKNOWN: u8 = 4;

    /// Encode typed status into the D-Bus wire value.
    pub fn to_wire(status: AuraMutationStatus) -> u8 {
        match status {
            AuraMutationStatus::Supported => SUPPORTED,
            AuraMutationStatus::Unsupported => UNSUPPORTED,
            AuraMutationStatus::TemporarilyUnavailable => TEMPORARILY_UNAVAILABLE,
            AuraMutationStatus::PermissionDenied => PERMISSION_DENIED,
            AuraMutationStatus::Unknown => UNKNOWN,
        }
    }

    /// Decode a wire value; unknown values produce `None` so callers classify
    /// them as `Unknown` instead of inventing a known state.
    pub fn from_wire(raw: u8) -> Option<AuraMutationStatus> {
        match raw {
            SUPPORTED => Some(AuraMutationStatus::Supported),
            UNSUPPORTED => Some(AuraMutationStatus::Unsupported),
            TEMPORARILY_UNAVAILABLE => Some(AuraMutationStatus::TemporarilyUnavailable),
            PERMISSION_DENIED => Some(AuraMutationStatus::PermissionDenied),
            UNKNOWN => Some(AuraMutationStatus::Unknown),
            _ => None,
        }
    }
}

#[async_trait]
pub trait AuraStaticRgbMutationBackend: Send + Sync {
    /// Perform exactly one Static RGB mutation with config-level read-back.
    async fn set_static_rgb(
        &self,
        rgb: AuraRgb,
    ) -> Result<AuraStaticRgbMutationReadback, ProviderError>;

    /// Perform one mode/speed/colour mutation with config-level read-back.
    async fn set_effect(
        &self,
        effect: AuraEffect,
    ) -> Result<AuraEffectMutationReadback, ProviderError>;

    /// Report the typed runtime availability of this mutation backend.
    ///
    /// This is read-only evidence used by capability probing; it never
    /// performs I/O and never mutates hardware.
    fn mutation_status(&self) -> AuraMutationStatus;
}

/// Internal compatibility backend; it never writes the kernel directly.
pub struct AsusdAuraStaticRgbMutationBackend<A> {
    asusd: A,
}

impl<A> AsusdAuraStaticRgbMutationBackend<A> {
    /// Construct without performing I/O.
    pub fn new(asusd: A) -> Self {
        Self { asusd }
    }
}

impl<A> AsusdAuraStaticRgbMutationBackend<A>
where
    A: AsusdAuraClient,
{
    /// Validate supported mode, preserve hardware-owned zone/direction, then
    /// confirm the complete requested effect through a fresh config read.
    pub async fn set_effect(
        &self,
        requested: AuraEffect,
    ) -> Result<AuraEffectMutationReadback, ProviderError> {
        let current = self.asusd.led_mode_data().await?;
        let supported = self.asusd.supported_basic_modes().await?;
        if !supported.contains(&requested.mode.to_u32()) {
            return Err(ProviderError::Unsupported(format!(
                "asus aura: mode {:?} not supported (supported_basic_modes={supported:?})",
                requested.mode
            )));
        }
        let effect = AuraEffect {
            mode: requested.mode,
            zone: current.zone,
            colour1: requested.colour1,
            colour2: requested.colour2,
            speed: requested.speed,
            direction: current.direction,
        };
        self.asusd.set_led_mode_data(effect.clone()).await?;
        let observed = self.asusd.led_mode_data().await?;
        if observed != effect {
            return Err(ProviderError::BackendUnavailable(format!(
                "asus aura config read-back mismatch: expected={effect:?}, got={observed:?}"
            )));
        }
        Ok(AuraEffectMutationReadback {
            requested: effect,
            observed,
            result: ApplyResult::Accepted,
        })
    }

    /// Validate, perform one asusd setter, then perform a fresh config-level
    /// read-back.
    pub async fn set_static_rgb(
        &self,
        rgb: AuraRgb,
    ) -> Result<AuraStaticRgbMutationReadback, ProviderError> {
        // 1. Fresh read current LedModeData.
        let current = self.asusd.led_mode_data().await?;

        // 2. Fresh read SupportedBasicModes; Static must be supported.
        let supported = self.asusd.supported_basic_modes().await?;
        if !supported.contains(&AuraMode::Static.to_u32()) {
            return Err(ProviderError::Unsupported(format!(
                "asus aura: Static mode not supported (supported_basic_modes={supported:?})"
            )));
        }

        // 3-4. Preserve zone/colour2/speed/direction; replace only primary RGB;
        //      mode is fixed to Static (this is a Static RGB mutation).
        let effect = AuraEffect {
            mode: AuraMode::Static,
            zone: current.zone,
            colour1: rgb,
            colour2: current.colour2,
            speed: current.speed,
            direction: current.direction,
        };

        // 5. Exactly one mutation, owned by asusd. No retry and no fallback.
        self.asusd.set_led_mode_data(effect).await?;

        // 6. Fresh config-level read-back.
        let observed = self.asusd.led_mode_data().await?;

        // 7. Confirm config-level RGB match.
        if observed.colour1 != rgb {
            return Err(ProviderError::BackendUnavailable(format!(
                "asus aura config read-back mismatch: expected={rgb:?}, got={:?}",
                observed.colour1
            )));
        }

        // 8. Honest result: hardware state not confirmed.
        Ok(AuraStaticRgbMutationReadback {
            requested: rgb,
            observed: observed.colour1,
            result: ApplyResult::Accepted,
        })
    }
}

#[async_trait]
impl<A> AuraStaticRgbMutationBackend for AsusdAuraStaticRgbMutationBackend<A>
where
    A: AsusdAuraClient,
{
    async fn set_static_rgb(
        &self,
        rgb: AuraRgb,
    ) -> Result<AuraStaticRgbMutationReadback, ProviderError> {
        self.set_static_rgb(rgb).await
    }

    async fn set_effect(
        &self,
        effect: AuraEffect,
    ) -> Result<AuraEffectMutationReadback, ProviderError> {
        AsusdAuraStaticRgbMutationBackend::set_effect(self, effect).await
    }

    fn mutation_status(&self) -> AuraMutationStatus {
        AuraMutationStatus::Supported
    }
}

/// Stable wire DTO for one Aura Static RGB mutation observation.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    zbus::zvariant::Type,
    zbus::zvariant::OwnedValue,
)]
pub struct AuraMutationResult {
    pub requested_r: u8,
    pub requested_g: u8,
    pub requested_b: u8,
    pub observed_r: u8,
    pub observed_g: u8,
    pub observed_b: u8,
    pub outcome: u32,
}

/// Outcome: asusd accepted and config-level read-back matched; hardware state
/// not confirmed (`kbd_rgb_mode` write-only).
pub const AURA_OUTCOME_CONFIG_CONFIRMED: u32 = 0;

/// Обработка Aura Static RGB mutation до публичного D-Bus boundary.
///
/// Wire-значения RGB (`u8` × 3) уже строгие типы, поэтому malformed вход
/// невозможен на этом уровне. После authorization выполняется ровно одна
/// typed mutation с config-level read-back; результат `Accepted` означает,
/// что hardware state не подтверждён (`kbd_rgb_mode` write-only).
pub async fn handle_set_aura_static_rgb(
    authorizer: &dyn Authorizer,
    backend: &dyn AuraStaticRgbMutationBackend,
    rgb: AuraRgb,
    sender: &str,
) -> zbus::fdo::Result<AuraMutationResult> {
    tracing::info!(?rgb, "aura_static_rgb Hardware1 request");
    authorizer.authorize(sender).await.map_err(|e| match e {
        AuthorizeError::Denied(msg) => zbus::fdo::Error::AccessDenied(msg),
        AuthorizeError::Failed(msg) => zbus::fdo::Error::Failed(msg),
    })?;

    let readback = backend
        .set_static_rgb(rgb)
        .await
        .map_err(provider_error_to_dbus)?;
    match readback.result {
        ApplyResult::Accepted => Ok(AuraMutationResult {
            requested_r: readback.requested.r,
            requested_g: readback.requested.g,
            requested_b: readback.requested.b,
            observed_r: readback.observed.r,
            observed_g: readback.observed.g,
            observed_b: readback.observed.b,
            outcome: AURA_OUTCOME_CONFIG_CONFIRMED,
        }),
        other => Err(zbus::fdo::Error::Failed(format!(
            "hardwared: aura static rgb operation not confirmed: {other:?}"
        ))),
    }
}

/// Apply a typed Aura effect through Hardware1 and require config read-back.
pub async fn handle_set_aura_effect(
    authorizer: &dyn Authorizer,
    backend: &dyn AuraStaticRgbMutationBackend,
    effect: AuraEffect,
    sender: &str,
) -> zbus::fdo::Result<AuraEffectMutationReadback> {
    authorizer.authorize(sender).await.map_err(|e| match e {
        AuthorizeError::Denied(msg) => zbus::fdo::Error::AccessDenied(msg),
        AuthorizeError::Failed(msg) => zbus::fdo::Error::Failed(msg),
    })?;
    let readback = backend
        .set_effect(effect)
        .await
        .map_err(provider_error_to_dbus)?;
    if readback.result == ApplyResult::Accepted {
        Ok(readback)
    } else {
        Err(zbus::fdo::Error::Failed(format!(
            "hardwared: aura effect operation not confirmed: {:?}",
            readback.result
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    use std::time::Duration;

    use super::*;
    use crate::{AuthorizeError, Authorizer};
    use orbis_core::aura::{AuraDirection, AuraSpeed};

    #[derive(Clone)]
    struct FakeAsusd {
        effect: Arc<Mutex<AuraEffect>>,
        supported_modes: Arc<Mutex<Vec<u32>>>,
        setter_calls: Arc<AtomicUsize>,
        setter_error: Arc<Mutex<Option<String>>>,
        readback_error: Arc<Mutex<Option<String>>>,
        getter_override: Arc<Mutex<Option<AuraEffect>>>,
        last_set: Arc<Mutex<Option<AuraEffect>>>,
    }

    impl FakeAsusd {
        fn new(effect: AuraEffect, supported_modes: Vec<u32>) -> Self {
            Self {
                effect: Arc::new(Mutex::new(effect)),
                supported_modes: Arc::new(Mutex::new(supported_modes)),
                setter_calls: Arc::new(AtomicUsize::new(0)),
                setter_error: Arc::new(Mutex::new(None)),
                readback_error: Arc::new(Mutex::new(None)),
                getter_override: Arc::new(Mutex::new(None)),
                last_set: Arc::new(Mutex::new(None)),
            }
        }

        fn setter_calls(&self) -> usize {
            self.setter_calls.load(Ordering::SeqCst)
        }

        fn last_set(&self) -> Option<AuraEffect> {
            self.last_set.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl AsusdAuraClient for FakeAsusd {
        async fn led_mode_data(&self) -> Result<AuraEffect, ProviderError> {
            // `readback_error` applies only to reads after the setter, so the
            // initial fresh read (step 1) still succeeds.
            if self.setter_calls.load(Ordering::SeqCst) > 0 {
                if let Some(error) = self.readback_error.lock().unwrap().clone() {
                    return Err(ProviderError::Dbus(error));
                }
            }
            Ok(self
                .getter_override
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| self.effect.lock().unwrap().clone()))
        }

        async fn supported_basic_modes(&self) -> Result<Vec<u32>, ProviderError> {
            Ok(self.supported_modes.lock().unwrap().clone())
        }

        async fn set_led_mode_data(&self, effect: AuraEffect) -> Result<(), ProviderError> {
            self.setter_calls.fetch_add(1, Ordering::SeqCst);
            if let Some(error) = self.setter_error.lock().unwrap().clone() {
                return Err(ProviderError::Dbus(error));
            }
            *self.last_set.lock().unwrap() = Some(effect.clone());
            *self.effect.lock().unwrap() = effect;
            Ok(())
        }
    }

    fn static_effect() -> AuraEffect {
        AuraEffect {
            mode: AuraMode::Static,
            zone: AuraZone::None,
            colour1: AuraRgb {
                r: 0xff,
                g: 0x11,
                b: 0xdd,
            },
            colour2: AuraRgb { r: 0, g: 0, b: 0 },
            speed: AuraSpeed::Med,
            direction: AuraDirection::Right,
        }
    }

    fn backend(asusd: FakeAsusd) -> AsusdAuraStaticRgbMutationBackend<FakeAsusd> {
        AsusdAuraStaticRgbMutationBackend::new(asusd)
    }

    #[tokio::test]
    async fn rgb_set_config_readback_match_is_accepted() {
        let asusd = FakeAsusd::new(static_effect(), vec![0]);
        let requested = AuraRgb {
            r: 0x12,
            g: 0x34,
            b: 0x56,
        };
        let result = backend(asusd.clone())
            .set_static_rgb(requested)
            .await
            .unwrap();
        assert_eq!(asusd.setter_calls(), 1);
        assert_eq!(result.requested, requested);
        assert_eq!(result.observed, requested);
        assert_eq!(result.result, ApplyResult::Accepted);
        assert!(
            !result.result.is_applied(),
            "Accepted must not claim hardware-applied"
        );
    }

    #[tokio::test]
    async fn effect_mutation_preserves_zone_direction_and_confirms_readback() {
        let asusd = FakeAsusd::new(static_effect(), vec![0, 1]);
        let requested = AuraEffect {
            mode: AuraMode::Breathe,
            zone: AuraZone::None,
            colour1: AuraRgb { r: 1, g: 2, b: 3 },
            colour2: AuraRgb { r: 4, g: 5, b: 6 },
            speed: AuraSpeed::High,
            direction: AuraDirection::Right,
        };

        let result = backend(asusd.clone())
            .set_effect(requested.clone())
            .await
            .unwrap();

        let sent = asusd.last_set().expect("setter must be called");
        assert_eq!(sent.mode, AuraMode::Breathe);
        assert_eq!(sent.speed, AuraSpeed::High);
        assert_eq!(sent.colour1, requested.colour1);
        assert_eq!(sent.colour2, requested.colour2);
        assert_eq!(sent.zone, AuraZone::None);
        assert_eq!(sent.direction, AuraDirection::Right);
        assert_eq!(result.observed, requested);
        assert_eq!(result.result, ApplyResult::Accepted);
    }

    #[tokio::test]
    async fn tuple_fields_preserved_and_mode_is_static() {
        let current = AuraEffect {
            mode: AuraMode::Breathe,
            zone: AuraZone::None,
            colour1: AuraRgb {
                r: 0xff,
                g: 0x00,
                b: 0x7c,
            },
            colour2: AuraRgb {
                r: 0x9b,
                g: 0x26,
                b: 0xb6,
            },
            speed: AuraSpeed::Low,
            direction: AuraDirection::Left,
        };
        let asusd = FakeAsusd::new(current, vec![0]);
        let requested = AuraRgb {
            r: 0x01,
            g: 0x02,
            b: 0x03,
        };
        let result = backend(asusd.clone())
            .set_static_rgb(requested)
            .await
            .unwrap();
        let sent = asusd.last_set().expect("setter must be called");
        assert_eq!(sent.mode, AuraMode::Static);
        assert_eq!(sent.zone, AuraZone::None);
        assert_eq!(sent.colour1, requested);
        assert_eq!(
            sent.colour2,
            AuraRgb {
                r: 0x9b,
                g: 0x26,
                b: 0xb6
            }
        );
        assert_eq!(sent.speed, AuraSpeed::Low);
        assert_eq!(sent.direction, AuraDirection::Left);
        assert_eq!(result.result, ApplyResult::Accepted);
    }

    #[tokio::test]
    async fn static_unsupported_no_write() {
        let asusd = FakeAsusd::new(static_effect(), vec![1]);
        let error = backend(asusd.clone())
            .set_static_rgb(AuraRgb { r: 1, g: 2, b: 3 })
            .await
            .expect_err("Static unsupported");
        assert!(matches!(error, ProviderError::Unsupported(_)));
        assert_eq!(asusd.setter_calls(), 0);
    }

    #[tokio::test]
    async fn setter_error_propagates() {
        let asusd = FakeAsusd::new(static_effect(), vec![0]);
        *asusd.setter_error.lock().unwrap() = Some("setter failed".into());
        let error = backend(asusd.clone())
            .set_static_rgb(AuraRgb { r: 1, g: 2, b: 3 })
            .await
            .expect_err("setter error");
        assert!(matches!(error, ProviderError::Dbus(_)));
        assert_eq!(asusd.setter_calls(), 1);
    }

    #[tokio::test]
    async fn readback_mismatch_is_error() {
        let asusd = FakeAsusd::new(static_effect(), vec![0]);
        *asusd.getter_override.lock().unwrap() = Some(AuraEffect {
            mode: AuraMode::Static,
            zone: AuraZone::None,
            colour1: AuraRgb {
                r: 0x99,
                g: 0x99,
                b: 0x99,
            },
            colour2: AuraRgb { r: 0, g: 0, b: 0 },
            speed: AuraSpeed::Med,
            direction: AuraDirection::Right,
        });
        let error = backend(asusd.clone())
            .set_static_rgb(AuraRgb { r: 1, g: 2, b: 3 })
            .await
            .expect_err("read-back mismatch");
        assert!(matches!(error, ProviderError::BackendUnavailable(_)));
        assert_eq!(asusd.setter_calls(), 1);
    }

    #[tokio::test]
    async fn readback_unavailable_is_not_optimistic_success() {
        let asusd = FakeAsusd::new(static_effect(), vec![0]);
        *asusd.readback_error.lock().unwrap() = Some("read-back unavailable".into());
        let result = backend(asusd.clone())
            .set_static_rgb(AuraRgb { r: 1, g: 2, b: 3 })
            .await;
        assert!(matches!(result, Err(ProviderError::Dbus(_))));
        assert_eq!(asusd.setter_calls(), 1);
        assert!(!matches!(result, Ok(readback) if readback.result == ApplyResult::Accepted));
    }

    #[derive(Clone, Copy)]
    enum AuthOutcome {
        Ok,
        Denied,
        Failed,
    }

    struct FakeAuthorizer {
        outcome: Arc<Mutex<AuthOutcome>>,
        calls: Arc<AtomicUsize>,
        last_sender: Arc<Mutex<Option<String>>>,
    }

    impl FakeAuthorizer {
        fn new(outcome: AuthOutcome) -> Self {
            Self {
                outcome: Arc::new(Mutex::new(outcome)),
                calls: Arc::new(AtomicUsize::new(0)),
                last_sender: Arc::new(Mutex::new(None)),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl Authorizer for FakeAuthorizer {
        async fn authorize(&self, sender: &str) -> Result<(), AuthorizeError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.last_sender.lock().unwrap() = Some(sender.to_string());
            match *self.outcome.lock().unwrap() {
                AuthOutcome::Ok => Ok(()),
                AuthOutcome::Denied => Err(AuthorizeError::Denied("denied".into())),
                AuthOutcome::Failed => Err(AuthorizeError::Failed("polkit down".into())),
            }
        }
    }

    #[tokio::test]
    async fn auth_denied_no_write() {
        let asusd = FakeAsusd::new(static_effect(), vec![0]);
        let backend = backend(asusd.clone());
        let auth = FakeAuthorizer::new(AuthOutcome::Denied);
        let error =
            handle_set_aura_static_rgb(&auth, &backend, AuraRgb { r: 1, g: 2, b: 3 }, ":1.42")
                .await
                .expect_err("denied");
        assert!(matches!(error, zbus::fdo::Error::AccessDenied(_)));
        assert_eq!(asusd.setter_calls(), 0);
        assert_eq!(auth.calls(), 1);
        assert_eq!(auth.last_sender.lock().unwrap().as_deref(), Some(":1.42"));
    }

    #[tokio::test]
    async fn auth_failed_no_write() {
        let asusd = FakeAsusd::new(static_effect(), vec![0]);
        let backend = backend(asusd.clone());
        let auth = FakeAuthorizer::new(AuthOutcome::Failed);
        let error =
            handle_set_aura_static_rgb(&auth, &backend, AuraRgb { r: 1, g: 2, b: 3 }, ":1.7")
                .await
                .expect_err("polkit down");
        assert!(matches!(error, zbus::fdo::Error::Failed(_)));
        assert_eq!(asusd.setter_calls(), 0);
    }

    #[tokio::test]
    async fn authorized_returns_config_confirmed_result() {
        let asusd = FakeAsusd::new(static_effect(), vec![0]);
        let backend = backend(asusd.clone());
        let auth = FakeAuthorizer::new(AuthOutcome::Ok);
        let result = handle_set_aura_static_rgb(
            &auth,
            &backend,
            AuraRgb {
                r: 0x12,
                g: 0x34,
                b: 0x56,
            },
            ":1.42",
        )
        .await
        .expect("authorized");
        assert_eq!(result.requested_r, 0x12);
        assert_eq!(result.requested_g, 0x34);
        assert_eq!(result.requested_b, 0x56);
        assert_eq!(result.observed_r, 0x12);
        assert_eq!(result.observed_g, 0x34);
        assert_eq!(result.observed_b, 0x56);
        assert_eq!(result.outcome, AURA_OUTCOME_CONFIG_CONFIRMED);
        assert_eq!(asusd.setter_calls(), 1);
    }

    #[tokio::test]
    async fn malformed_current_tuple_is_error() {
        tokio::time::timeout(Duration::from_secs(5), async {
            struct MalformedAura;
            #[zbus::interface(name = "xyz.ljones.Aura")]
            impl MalformedAura {
                #[zbus(property)]
                async fn led_mode_data(&self) -> String {
                    "malformed".to_string()
                }
            }

            let (server_stream, client_stream) = std::os::unix::net::UnixStream::pair().unwrap();
            let guid = zbus::Guid::generate();
            let server_builder = zbus::connection::Builder::unix_stream(server_stream)
                .server(guid)
                .unwrap()
                .p2p()
                .name(ASUSD_AURA_DESTINATION)
                .unwrap()
                .serve_at(ASUSD_AURA_PATH, MalformedAura)
                .unwrap();
            let client_builder = zbus::connection::Builder::unix_stream(client_stream).p2p();
            let (_server, client) =
                tokio::try_join!(server_builder.build(), client_builder.build()).expect("p2p");

            let asusd = ZbusAsusdAuraClient::new(client);
            let error = asusd
                .led_mode_data()
                .await
                .expect_err("malformed payload must error");
            assert!(
                matches!(error, ProviderError::Dbus(_)),
                "expected Dbus, got {error:?}"
            );
        })
        .await
        .expect("p2p test timeout");
    }

    #[test]
    fn effect_wire_roundtrip_preserves_unknown() {
        let effect = AuraEffect {
            mode: AuraMode::Unknown(42),
            zone: AuraZone::Unknown(9),
            colour1: AuraRgb { r: 1, g: 2, b: 3 },
            colour2: AuraRgb { r: 4, g: 5, b: 6 },
            speed: AuraSpeed::Unknown("Turbo".to_string()),
            direction: AuraDirection::Unknown("Diagonal".to_string()),
        };
        assert_eq!(effect_from_wire(effect_to_wire(&effect)), effect);
    }

    #[test]
    fn aura_mutation_wire_roundtrip_is_total() {
        for status in [
            AuraMutationStatus::Supported,
            AuraMutationStatus::Unsupported,
            AuraMutationStatus::TemporarilyUnavailable,
            AuraMutationStatus::PermissionDenied,
            AuraMutationStatus::Unknown,
        ] {
            let wire = aura_mutation_wire::to_wire(status);
            assert_eq!(aura_mutation_wire::from_wire(wire), Some(status));
        }
        assert_eq!(aura_mutation_wire::from_wire(99), None);
    }

    #[test]
    fn production_contract_is_typed_and_not_shell_based() {
        assert_eq!(ASUSD_AURA_DESTINATION, "xyz.ljones.Asusd");
        assert_eq!(ASUSD_AURA_PATH, "/xyz/ljones/aura/tuf");
        assert_eq!(ASUSD_AURA_INTERFACE, "xyz.ljones.Aura");
    }
}
