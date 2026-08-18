//! Read-only ASUS Aura RGB provider over the asusd D-Bus interface.
//!
//! Authority: asusd service `xyz.ljones.Asusd`, interface `xyz.ljones.Aura`,
//! object path `/xyz/ljones/aura/tuf` (TUF laptop keyboard).
//!
//! Read-only: this provider never calls any setter and performs no hardware
//! writes. Wire contract mirrors upstream asusctl `rog_aura` types:
//! - `led_mode` → u32 (`AuraModeNum`);
//! - `led_mode_data` → struct `(uu(yyy)(yyy)ss)` (`AuraEffect`);
//! - `supported_basic_modes` → array of u32;
//! - `supported_basic_zones` → array of u32 (empty is valid for single-zone TUF);
//! - `brightness` → u32 (`LedBrightness`);
//! - `supported_brightness` → array of u32.
//!
//! Semantics:
//! - service/path/interface absent → `Unsupported` (service missing →
//!   `BackendUnavailable`);
//! - malformed D-Bus payload (wrong type/structure) → error, never a default;
//! - unknown enum wire values preserved as `Unknown`, never coerced to a known
//!   state.

use std::time::Duration;

use async_trait::async_trait;
use orbis_core::aura::{AuraBrightness, AuraEffect, AuraMode, AuraRgb, AuraState, AuraZone};
use orbis_core::diagnostics::DiagnosticEntry;
use orbis_core::identity::BackendIdentity;
use zbus::proxy::Builder;
use zbus::zvariant::{OwnedValue, Value};

use crate::error::ProviderError;
use crate::traits::{AuraProvider, Provider, ProviderHealth};

/// asusd well-known D-Bus name.
pub const ASUS_AURA_DESTINATION: &str = "xyz.ljones.Asusd";
/// Aura object path for TUF laptop keyboards.
pub const ASUS_AURA_PATH: &str = "/xyz/ljones/aura/tuf";
/// Aura D-Bus interface.
pub const ASUS_AURA_INTERFACE: &str = "xyz.ljones.Aura";

/// Read-only Aura RGB provider over asusd.
pub struct AsusAuraProvider {
    connection: zbus::Connection,
    destination: String,
    path: String,
    interface: String,
}

impl AsusAuraProvider {
    /// Создать provider над готовой session/system-bus Connection.
    pub fn new(connection: zbus::Connection) -> Self {
        Self {
            connection,
            destination: ASUS_AURA_DESTINATION.to_string(),
            path: ASUS_AURA_PATH.to_string(),
            interface: ASUS_AURA_INTERFACE.to_string(),
        }
    }

    /// Прочитать property как `OwnedValue` (fresh, без property cache).
    async fn get_property(&self, name: &str) -> Result<Value<'static>, ProviderError> {
        let proxy = Builder::<zbus::Proxy>::new(&self.connection)
            .destination(self.destination.clone())
            .map_err(|e| ProviderError::Internal(format!("asus aura: invalid destination: {e}")))?
            .path(self.path.clone())
            .map_err(|e| ProviderError::Internal(format!("asus aura: invalid path: {e}")))?
            .interface(self.interface.clone())
            .map_err(|e| ProviderError::Internal(format!("asus aura: invalid interface: {e}")))?
            .build()
            .await
            .map_err(|e| map_aura_dbus_error(&self.path, e))?;
        let owned: OwnedValue = proxy
            .get_property(name)
            .await
            .map_err(|e| map_aura_dbus_error(&self.path, e))?;
        Ok(owned.into())
    }

    /// Прочитать свойство как u32.
    async fn get_u32(&self, name: &str) -> Result<u32, ProviderError> {
        match self.get_property(name).await? {
            Value::U32(value) => Ok(value),
            other => Err(ProviderError::Internal(format!(
                "asus aura: property '{name}' malformed: expected u32, got {other:?}"
            ))),
        }
    }

    /// Прочитать свойство как массив u32.
    async fn get_u32_array(&self, name: &str) -> Result<Vec<u32>, ProviderError> {
        match self.get_property(name).await? {
            Value::Array(array) => {
                let mut values = Vec::with_capacity(array.len());
                for element in array.iter() {
                    match element {
                        Value::U32(value) => values.push(*value),
                        other => {
                            return Err(ProviderError::Internal(format!(
                                "asus aura: property '{name}' malformed: array element \
                                 expected u32, got {other:?}"
                            )));
                        }
                    }
                }
                Ok(values)
            }
            other => Err(ProviderError::Internal(format!(
                "asus aura: property '{name}' malformed: expected array, got {other:?}"
            ))),
        }
    }

    /// Прочитать `LedModeData` (struct `(uu(yyy)(yyy)ss)`) в `AuraEffect`.
    async fn get_effect(&self) -> Result<AuraEffect, ProviderError> {
        let value = self.get_property("LedModeData").await?;
        parse_effect(value)
    }
}

/// Разобрать `AuraEffect` из wire-структуры `(uu(yyy)(yyy)ss)`.
///
/// Malformed структура → `Internal`, не default.
fn parse_effect(value: Value<'static>) -> Result<AuraEffect, ProviderError> {
    let Value::Structure(structure) = value else {
        return Err(ProviderError::Internal(format!(
            "asus aura: led_mode_data malformed: expected structure, got {value:?}"
        )));
    };
    let fields = structure.fields();
    if fields.len() != 6 {
        return Err(ProviderError::Internal(format!(
            "asus aura: led_mode_data malformed: expected 6 fields, got {}",
            fields.len()
        )));
    }
    let mode = match fields[0] {
        Value::U32(value) => AuraMode::from_u32(value),
        ref other => {
            return Err(ProviderError::Internal(format!(
                "asus aura: led_mode_data malformed: mode expected u32, got {other:?}"
            )));
        }
    };
    let zone = match fields[1] {
        Value::U32(value) => AuraZone::from_u32(value),
        ref other => {
            return Err(ProviderError::Internal(format!(
                "asus aura: led_mode_data malformed: zone expected u32, got {other:?}"
            )));
        }
    };
    let colour1 = parse_colour(&fields[2], "colour1")?;
    let colour2 = parse_colour(&fields[3], "colour2")?;
    let speed = match &fields[4] {
        Value::Str(value) => value.as_str().parse().unwrap(),
        other => {
            return Err(ProviderError::Internal(format!(
                "asus aura: led_mode_data malformed: speed expected string, got {other:?}"
            )));
        }
    };
    let direction = match &fields[5] {
        Value::Str(value) => value.as_str().parse().unwrap(),
        other => {
            return Err(ProviderError::Internal(format!(
                "asus aura: led_mode_data malformed: direction expected string, got {other:?}"
            )));
        }
    };
    Ok(AuraEffect {
        mode,
        zone,
        colour1,
        colour2,
        speed,
        direction,
    })
}

/// Разобрать `Colour` из wire-структуры `(yyy)`.
fn parse_colour(value: &Value<'static>, name: &str) -> Result<AuraRgb, ProviderError> {
    let Value::Structure(structure) = value else {
        return Err(ProviderError::Internal(format!(
            "asus aura: led_mode_data malformed: {name} expected structure, got {value:?}"
        )));
    };
    let fields = structure.fields();
    if fields.len() != 3 {
        return Err(ProviderError::Internal(format!(
            "asus aura: led_mode_data malformed: {name} expected 3 fields, got {}",
            fields.len()
        )));
    }
    let mut channels = [0u8; 3];
    for (index, channel) in channels.iter_mut().enumerate() {
        match fields[index] {
            Value::U8(value) => *channel = value,
            ref other => {
                return Err(ProviderError::Internal(format!(
                    "asus aura: led_mode_data malformed: {name} channel {index} expected u8, \
                     got {other:?}"
                )));
            }
        }
    }
    Ok(AuraRgb {
        r: channels[0],
        g: channels[1],
        b: channels[2],
    })
}

/// Map a D-Bus/zbus error into a typed provider error.
///
/// - service missing (`ServiceUnknown`/`NameNotFound`) → `BackendUnavailable`;
/// - object path / interface / method absent → `Unsupported`;
/// - access denied → `PermissionDenied`;
/// - anything else → `Dbus`.
fn map_aura_dbus_error(path: &str, error: zbus::Error) -> ProviderError {
    match &error {
        zbus::Error::MethodError(name, _, _) => match name.as_str() {
            "org.freedesktop.DBus.Error.ServiceUnknown" => ProviderError::BackendUnavailable(
                format!("asus aura: service absent: {ASUS_AURA_DESTINATION}"),
            ),
            "org.freedesktop.DBus.Error.UnknownObject" => {
                ProviderError::Unsupported(format!("asus aura: Aura object absent: {path}"))
            }
            "org.freedesktop.DBus.Error.UnknownInterface"
            | "org.freedesktop.DBus.Error.UnknownMethod" => {
                ProviderError::Unsupported(format!("asus aura: Aura interface absent on {path}"))
            }
            "org.freedesktop.DBus.Error.AccessDenied" => {
                ProviderError::PermissionDenied(format!("asus aura: read denied on {path}"))
            }
            _ => ProviderError::Dbus(error.to_string()),
        },
        zbus::Error::InterfaceNotFound => {
            ProviderError::Unsupported(format!("asus aura: interface absent on {path}"))
        }
        _ => ProviderError::Dbus(error.to_string()),
    }
}

impl Provider for AsusAuraProvider {
    fn id(&self) -> &'static str {
        "asus-aura"
    }

    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("asus-aura")
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }

    fn explain_unsupported(&self, feature: &str) -> String {
        format!("asus aura: функция '{feature}' недоступна")
    }

    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }

    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        vec![DiagnosticEntry::new(
            "provider.asus-aura",
            "read-only asusd Aura RGB backend",
        )]
    }
}

#[async_trait]
impl AuraProvider for AsusAuraProvider {
    async fn aura_state(&self) -> Result<AuraState, ProviderError> {
        let current_mode = AuraMode::from_u32(self.get_u32("LedMode").await?);
        let current_effect = self.get_effect().await?;
        let brightness = AuraBrightness::from_u32(self.get_u32("Brightness").await?);
        let supported_modes = self
            .get_u32_array("SupportedBasicModes")
            .await?
            .into_iter()
            .map(AuraMode::from_u32)
            .collect();
        let supported_zones = self
            .get_u32_array("SupportedBasicZones")
            .await?
            .into_iter()
            .map(AuraZone::from_u32)
            .collect();
        let supported_brightness = self
            .get_u32_array("SupportedBrightness")
            .await?
            .into_iter()
            .map(AuraBrightness::from_u32)
            .collect();
        Ok(AuraState {
            current_mode,
            current_effect,
            brightness,
            supported_modes,
            supported_zones,
            supported_brightness,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use orbis_core::aura::{AuraDirection, AuraSpeed};
    use zbus::connection::Builder;

    /// Wire tuple of `LedModeData`: struct `(uu(yyy)(yyy)ss)`.
    type EffectWire = (u32, u32, (u8, u8, u8), (u8, u8, u8), String, String);

    /// Fake `xyz.ljones.Aura` server for private p2p tests.
    struct FakeAura {
        setter_calls: Arc<AtomicUsize>,
        mode: u32,
        effect: EffectWire,
        supported_modes: Vec<u32>,
        supported_zones: Vec<u32>,
        brightness: u32,
        supported_brightness: Vec<u32>,
    }

    #[zbus::interface(name = "xyz.ljones.Aura")]
    impl FakeAura {
        #[zbus(property)]
        async fn led_mode(&self) -> u32 {
            self.mode
        }

        #[zbus(property)]
        async fn led_mode_data(&self) -> EffectWire {
            self.effect.clone()
        }

        #[zbus(property)]
        async fn supported_basic_modes(&self) -> Vec<u32> {
            self.supported_modes.clone()
        }

        #[zbus(property)]
        async fn supported_basic_zones(&self) -> Vec<u32> {
            self.supported_zones.clone()
        }

        #[zbus(property)]
        async fn brightness(&self) -> u32 {
            self.brightness
        }

        #[zbus(property)]
        async fn supported_brightness(&self) -> Vec<u32> {
            self.supported_brightness.clone()
        }

        /// Setters exist only to prove the provider never calls them.
        #[zbus(property)]
        async fn set_led_mode(&mut self, _mode: u32) -> Result<(), zbus::fdo::Error> {
            self.setter_calls.fetch_add(1, Ordering::SeqCst);
            Err(zbus::fdo::Error::NotSupported("no writes in tests".into()))
        }

        #[zbus(property)]
        async fn set_led_mode_data(&mut self, _effect: EffectWire) -> Result<(), zbus::fdo::Error> {
            self.setter_calls.fetch_add(1, Ordering::SeqCst);
            Err(zbus::fdo::Error::NotSupported("no writes in tests".into()))
        }

        #[zbus(property)]
        async fn set_brightness(&mut self, _brightness: u32) -> Result<(), zbus::fdo::Error> {
            self.setter_calls.fetch_add(1, Ordering::SeqCst);
            Err(zbus::fdo::Error::NotSupported("no writes in tests".into()))
        }
    }

    /// Fake object with a *different* interface name on the Aura path, used to
    /// prove interface-absence maps to `Unsupported`.
    struct NotAura;
    #[zbus::interface(name = "io.github.orbiscontrol.NotAura")]
    impl NotAura {}

    /// Fake `xyz.ljones.Aura` object returning a malformed `led_mode` (string
    /// instead of u32), used to prove malformed payloads error.
    struct MalformedAura;
    #[zbus::interface(name = "xyz.ljones.Aura")]
    impl MalformedAura {
        #[zbus(property)]
        async fn led_mode(&self) -> String {
            "Static".to_string()
        }
    }

    fn static_effect() -> EffectWire {
        (
            0,
            0,
            (0xff, 0x11, 0xdd),
            (0, 0, 0),
            "Med".to_string(),
            "Right".to_string(),
        )
    }

    async fn connect(
        server: FakeAura,
    ) -> Result<(zbus::Connection, zbus::Connection), Box<dyn std::error::Error + Send + Sync>>
    {
        let (server_stream, client_stream) = std::os::unix::net::UnixStream::pair()?;
        let guid = zbus::Guid::generate();
        let server_builder = Builder::unix_stream(server_stream)
            .server(guid)?
            .p2p()
            .name(ASUS_AURA_DESTINATION)?
            .serve_at(ASUS_AURA_PATH, server)?;
        let client_builder = Builder::unix_stream(client_stream).p2p();
        Ok(tokio::try_join!(
            server_builder.build(),
            client_builder.build()
        )?)
    }

    #[tokio::test]
    async fn static_mode_read_correctly() {
        tokio::time::timeout(Duration::from_secs(5), async {
            let server = FakeAura {
                setter_calls: Arc::new(AtomicUsize::new(0)),
                mode: 0,
                effect: static_effect(),
                supported_modes: vec![0],
                supported_zones: vec![],
                brightness: 3,
                supported_brightness: vec![0, 1, 2, 3],
            };
            let (_server, client) = connect(server).await.expect("p2p");
            let provider = AsusAuraProvider::new(client);

            let state = provider.aura_state().await.expect("state");
            assert_eq!(state.current_mode, AuraMode::Static);
            assert_eq!(state.current_effect.mode, AuraMode::Static);
            assert_eq!(state.current_effect.zone, AuraZone::None);
            assert_eq!(state.current_effect.speed, AuraSpeed::Med);
            assert_eq!(state.current_effect.direction, AuraDirection::Right);
            assert_eq!(state.brightness, AuraBrightness::High);
            assert_eq!(state.supported_modes, vec![AuraMode::Static]);
            assert_eq!(state.supported_zones, Vec::<AuraZone>::new());
            assert_eq!(
                state.supported_brightness,
                vec![
                    AuraBrightness::Off,
                    AuraBrightness::Low,
                    AuraBrightness::Med,
                    AuraBrightness::High,
                ]
            );
        })
        .await
        .expect("p2p test timeout");
    }

    #[tokio::test]
    async fn rgb_preserved_losslessly() {
        tokio::time::timeout(Duration::from_secs(5), async {
            let server = FakeAura {
                setter_calls: Arc::new(AtomicUsize::new(0)),
                mode: 0,
                effect: (
                    0,
                    0,
                    (0xff, 0x00, 0x7c),
                    (0x9b, 0x26, 0xb6),
                    "Low".into(),
                    "Left".into(),
                ),
                supported_modes: vec![0],
                supported_zones: vec![],
                brightness: 2,
                supported_brightness: vec![0, 1, 2, 3],
            };
            let (_server, client) = connect(server).await.expect("p2p");
            let provider = AsusAuraProvider::new(client);

            let state = provider.aura_state().await.expect("state");
            assert_eq!(
                state.current_effect.colour1,
                AuraRgb {
                    r: 0xff,
                    g: 0x00,
                    b: 0x7c
                }
            );
            assert_eq!(
                state.current_effect.colour2,
                AuraRgb {
                    r: 0x9b,
                    g: 0x26,
                    b: 0xb6
                }
            );
            assert_eq!(state.current_effect.speed, AuraSpeed::Low);
            assert_eq!(state.current_effect.direction, AuraDirection::Left);
        })
        .await
        .expect("p2p test timeout");
    }

    #[tokio::test]
    async fn supported_modes_zero_only() {
        tokio::time::timeout(Duration::from_secs(5), async {
            let server = FakeAura {
                setter_calls: Arc::new(AtomicUsize::new(0)),
                mode: 0,
                effect: static_effect(),
                supported_modes: vec![0],
                supported_zones: vec![],
                brightness: 3,
                supported_brightness: vec![0, 1, 2, 3],
            };
            let (_server, client) = connect(server).await.expect("p2p");
            let provider = AsusAuraProvider::new(client);

            let state = provider.aura_state().await.expect("state");
            assert_eq!(state.supported_modes, vec![AuraMode::Static]);
            assert_eq!(state.supported_modes.len(), 1);
        })
        .await
        .expect("p2p test timeout");
    }

    #[tokio::test]
    async fn empty_zones_is_valid_single_zone_tuf() {
        tokio::time::timeout(Duration::from_secs(5), async {
            let server = FakeAura {
                setter_calls: Arc::new(AtomicUsize::new(0)),
                mode: 0,
                effect: static_effect(),
                supported_modes: vec![0],
                supported_zones: vec![],
                brightness: 3,
                supported_brightness: vec![0, 1, 2, 3],
            };
            let (_server, client) = connect(server).await.expect("p2p");
            let provider = AsusAuraProvider::new(client);

            let state = provider.aura_state().await.expect("state");
            assert!(state.supported_zones.is_empty());
        })
        .await
        .expect("p2p test timeout");
    }

    #[tokio::test]
    async fn missing_aura_interface_is_unsupported() {
        tokio::time::timeout(Duration::from_secs(5), async {
            let (server_stream, client_stream) = std::os::unix::net::UnixStream::pair().unwrap();
            let guid = zbus::Guid::generate();
            let server_builder = Builder::unix_stream(server_stream)
                .server(guid)
                .unwrap()
                .p2p()
                .name(ASUS_AURA_DESTINATION)
                .unwrap()
                .serve_at(ASUS_AURA_PATH, NotAura)
                .unwrap();
            let client_builder = Builder::unix_stream(client_stream).p2p();
            let (_server, client) =
                tokio::try_join!(server_builder.build(), client_builder.build()).expect("p2p");
            let provider = AsusAuraProvider::new(client);

            let error = provider
                .aura_state()
                .await
                .expect_err("interface absent must error");
            assert!(
                matches!(error, ProviderError::Unsupported(_)),
                "expected Unsupported, got {error:?}"
            );
        })
        .await
        .expect("p2p test timeout");
    }

    #[tokio::test]
    async fn malformed_led_mode_is_error() {
        tokio::time::timeout(Duration::from_secs(5), async {
            let (server_stream, client_stream) = std::os::unix::net::UnixStream::pair().unwrap();
            let guid = zbus::Guid::generate();
            let server_builder = Builder::unix_stream(server_stream)
                .server(guid)
                .unwrap()
                .p2p()
                .name(ASUS_AURA_DESTINATION)
                .unwrap()
                .serve_at(ASUS_AURA_PATH, MalformedAura)
                .unwrap();
            let client_builder = Builder::unix_stream(client_stream).p2p();
            let (_server, client) =
                tokio::try_join!(server_builder.build(), client_builder.build()).expect("p2p");
            let provider = AsusAuraProvider::new(client);

            let error = provider
                .aura_state()
                .await
                .expect_err("malformed payload must error");
            assert!(
                matches!(error, ProviderError::Internal(_)),
                "expected Internal, got {error:?}"
            );
        })
        .await
        .expect("p2p test timeout");
    }

    #[tokio::test]
    async fn provider_never_calls_setters() {
        tokio::time::timeout(Duration::from_secs(5), async {
            let setter_calls = Arc::new(AtomicUsize::new(0));
            let server = FakeAura {
                setter_calls: setter_calls.clone(),
                mode: 0,
                effect: static_effect(),
                supported_modes: vec![0],
                supported_zones: vec![],
                brightness: 3,
                supported_brightness: vec![0, 1, 2, 3],
            };
            let (_server, client) = connect(server).await.expect("p2p");
            let provider = AsusAuraProvider::new(client);

            provider.aura_state().await.expect("state");
            assert_eq!(
                setter_calls.load(Ordering::SeqCst),
                0,
                "read-only provider must never call setters"
            );
        })
        .await
        .expect("p2p test timeout");
    }

    #[test]
    fn production_constants_are_fixed() {
        assert_eq!(ASUS_AURA_DESTINATION, "xyz.ljones.Asusd");
        assert_eq!(ASUS_AURA_PATH, "/xyz/ljones/aura/tuf");
        assert_eq!(ASUS_AURA_INTERFACE, "xyz.ljones.Aura");
    }
}
