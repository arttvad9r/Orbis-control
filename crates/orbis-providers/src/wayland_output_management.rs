//! Read-only discovery of the compositor's wlr output-management protocol.
//!
//! This module is intentionally **not** a DisplayRefresh mutation owner. It
//! proves only that the current Wayland compositor advertises
//! `zwlr_output_manager_v1` and records the advertised protocol version. Global
//! presence is necessary transport evidence for a future wlroots/COSMIC-style
//! owner, but it is not evidence of:
//!
//! - an internal-panel target identity;
//! - exact Auto/60/120 target availability;
//! - permission to apply a configuration;
//! - successful mutation/read-back semantics.
//!
//! Therefore callers must keep `FeatureId::DisplayRefresh.write` unsupported
//! until a concrete typed owner binds the protocol, proves its target and passes
//! executable mutation/read-back validation.

use async_trait::async_trait;

use crate::error::ProviderError;

/// Stable Wayland registry interface name for wlr output management.
pub const WLR_OUTPUT_MANAGER_INTERFACE: &str = "zwlr_output_manager_v1";

/// Highest protocol version understood by the current design contract.
///
/// Version 4 adds adaptive-sync state but keeps the configuration semantics used
/// by the planned refresh owner. The probe does not bind even this version; it
/// only records compatibility.
pub const WLR_OUTPUT_MANAGER_CLIENT_MAX_VERSION: u32 = 4;

/// Minimal registry observation used by the pure classifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaylandRegistryGlobal {
    /// Registry interface name.
    pub interface: String,
    /// Compositor-advertised interface version.
    pub version: u32,
}

/// Proven transport-level availability of `zwlr_output_manager_v1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WlrOutputManagementSupport {
    advertised_version: u32,
    compatible_version: u32,
}

impl WlrOutputManagementSupport {
    /// Exact compositor-advertised version.
    pub fn advertised_version(self) -> u32 {
        self.advertised_version
    }

    /// Version ceiling the current Orbis design may eventually bind.
    pub fn compatible_version(self) -> u32 {
        self.compatible_version
    }
}

/// Classify one immutable Wayland registry snapshot.
///
/// Absence is `Unsupported`; multiple manager globals or a zero version are
/// contradictory evidence and fail closed as `Internal` rather than selecting a
/// manager arbitrarily.
pub fn classify_wlr_output_management_globals(
    globals: &[WaylandRegistryGlobal],
) -> Result<WlrOutputManagementSupport, ProviderError> {
    let matches = globals
        .iter()
        .filter(|global| global.interface == WLR_OUTPUT_MANAGER_INTERFACE)
        .collect::<Vec<_>>();

    let manager = match matches.as_slice() {
        [] => {
            return Err(ProviderError::Unsupported(
                "compositor does not advertise zwlr_output_manager_v1".into(),
            ));
        }
        [manager] => *manager,
        _ => {
            return Err(ProviderError::Internal(
                "Wayland registry advertises multiple zwlr_output_manager_v1 globals; refusing ambiguous configuration owner"
                    .into(),
            ));
        }
    };

    if manager.version == 0 {
        return Err(ProviderError::Internal(
            "Wayland registry advertised zwlr_output_manager_v1 version 0".into(),
        ));
    }

    Ok(WlrOutputManagementSupport {
        advertised_version: manager.version,
        compatible_version: manager
            .version
            .min(WLR_OUTPUT_MANAGER_CLIENT_MAX_VERSION),
    })
}

/// Testable source of transport-level wlr output-management support.
#[async_trait]
pub trait WlrOutputManagementSource: Send + Sync {
    /// Read one authoritative registry snapshot and classify manager support.
    async fn output_management_support(
        &self,
    ) -> Result<WlrOutputManagementSupport, ProviderError>;
}

/// Production Wayland registry probe.
///
/// Uses the same direct `wayland-client` transport as the existing read-only
/// `wl_output` provider. It does not bind `zwlr_output_manager_v1`, create a
/// configuration, enable/disable a head, set a mode or call apply/test.
#[derive(Debug, Clone, Copy, Default)]
pub struct WaylandWlrOutputManagementSource;

#[derive(Default)]
struct WlrRegistryProbeState;

impl wayland_client::Dispatch<wayland_client::protocol::wl_registry::WlRegistry, ()>
    for WlrRegistryProbeState
{
    fn event(
        _state: &mut Self,
        _proxy: &wayland_client::protocol::wl_registry::WlRegistry,
        _event: wayland_client::protocol::wl_registry::Event,
        _data: &(),
        _conn: &wayland_client::Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
    ) {
    }
}

impl
    wayland_client::Dispatch<
        wayland_client::protocol::wl_registry::WlRegistry,
        wayland_client::globals::GlobalListContents,
    > for WlrRegistryProbeState
{
    fn event(
        _state: &mut Self,
        _proxy: &wayland_client::protocol::wl_registry::WlRegistry,
        _event: wayland_client::protocol::wl_registry::Event,
        _data: &wayland_client::globals::GlobalListContents,
        _conn: &wayland_client::Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
    ) {
    }
}

impl WaylandWlrOutputManagementSource {
    /// Perform the synchronous Wayland registry read.
    pub fn read_support_blocking(&self) -> Result<WlrOutputManagementSupport, ProviderError> {
        let conn = wayland_client::Connection::connect_to_env()
            .map_err(|error| ProviderError::BackendUnavailable(format!("wayland connect: {error}")))?;
        let (globals, _queue) =
            wayland_client::globals::registry_queue_init::<WlrRegistryProbeState>(&conn).map_err(
                |error| {
                    ProviderError::BackendUnavailable(format!(
                        "wayland registry init for output-management probe: {error}"
                    ))
                },
            )?;

        use wayland_client::Proxy;
        let global_list = globals
            .registry()
            .data::<wayland_client::globals::GlobalListContents>()
            .ok_or_else(|| {
                ProviderError::Internal(
                    "wayland registry data missing during output-management probe".into(),
                )
            })?;

        let observed = global_list
            .clone_list()
            .into_iter()
            .map(|global| WaylandRegistryGlobal {
                interface: global.interface,
                version: global.version,
            })
            .collect::<Vec<_>>();

        classify_wlr_output_management_globals(&observed)
    }
}

#[async_trait]
impl WlrOutputManagementSource for WaylandWlrOutputManagementSource {
    async fn output_management_support(
        &self,
    ) -> Result<WlrOutputManagementSupport, ProviderError> {
        let source = *self;
        tokio::task::spawn_blocking(move || source.read_support_blocking())
            .await
            .map_err(|error| {
                ProviderError::Internal(format!(
                    "wayland output-management probe task failed: {error}"
                ))
            })?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn global(interface: &str, version: u32) -> WaylandRegistryGlobal {
        WaylandRegistryGlobal {
            interface: interface.into(),
            version,
        }
    }

    #[test]
    fn missing_manager_is_explicitly_unsupported() {
        assert!(matches!(
            classify_wlr_output_management_globals(&[global("wl_output", 4)]),
            Err(ProviderError::Unsupported(_))
        ));
    }

    #[test]
    fn advertised_version_is_preserved_and_compatible_version_is_bounded() {
        let support = classify_wlr_output_management_globals(&[
            global("wl_output", 4),
            global(WLR_OUTPUT_MANAGER_INTERFACE, 7),
        ])
        .unwrap();
        assert_eq!(support.advertised_version(), 7);
        assert_eq!(support.compatible_version(), 4);
    }

    #[test]
    fn older_nonzero_manager_version_remains_transport_evidence() {
        let support = classify_wlr_output_management_globals(&[global(
            WLR_OUTPUT_MANAGER_INTERFACE,
            2,
        )])
        .unwrap();
        assert_eq!(support.advertised_version(), 2);
        assert_eq!(support.compatible_version(), 2);
    }

    #[test]
    fn zero_version_fails_closed() {
        assert!(matches!(
            classify_wlr_output_management_globals(&[global(
                WLR_OUTPUT_MANAGER_INTERFACE,
                0,
            )]),
            Err(ProviderError::Internal(_))
        ));
    }

    #[test]
    fn duplicate_manager_globals_are_not_selected_arbitrarily() {
        assert!(matches!(
            classify_wlr_output_management_globals(&[
                global(WLR_OUTPUT_MANAGER_INTERFACE, 4),
                global(WLR_OUTPUT_MANAGER_INTERFACE, 4),
            ]),
            Err(ProviderError::Internal(_))
        ));
    }

    #[test]
    fn production_probe_has_no_configuration_or_process_surface() {
        let source = include_str!("wayland_output_management.rs");
        let forbidden = [
            ["enable_", "head("].concat(),
            ["disable_", "head("].concat(),
            ["set_", "mode("].concat(),
            ["create_", "configuration("].concat(),
            ["Command", "::new("].concat(),
            "wlr-randr".to_string(),
            "kscreen-doctor".to_string(),
        ];
        for needle in forbidden {
            assert!(!source.contains(&needle), "unexpected mutation/process surface: {needle}");
        }
        assert!(!source.contains("unsafe"));
        assert!(source.contains("registry_queue_init"));
        assert!(source.contains(WLR_OUTPUT_MANAGER_INTERFACE));
    }
}
