//! Read-only D-Bus service-presence diagnostics.

use std::time::SystemTime;

use async_trait::async_trait;
use orbis_core::diagnostics::{
    DiagnosticsServiceId, ServiceAvailability, ServiceBusScope, ServiceCriticality,
    ServiceDiagnostics,
};
use zbus::{Connection, fdo::DBusProxy, names::BusName};

/// Exact well-known name of the Orbis privileged Hardware1 service.
pub const ORBIS_HARDWARE_BUS_NAME: &str = "io.github.orbiscontrol.Hardware";
/// Exact well-known name of the Orbis Session1 service.
pub const ORBIS_SESSION_BUS_NAME: &str = "io.github.orbiscontrol.Session";
/// Exact well-known name of asusd.
pub const ASUSD_BUS_NAME: &str = "xyz.ljones.Asusd";
/// Exact well-known name of supergfxd.
pub const SUPERGFXD_BUS_NAME: &str = "org.supergfxctl.Daemon";

/// One fixed service-presence query target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServicePresenceTarget {
    service: DiagnosticsServiceId,
    bus: ServiceBusScope,
    bus_name: &'static str,
}

impl ServicePresenceTarget {
    /// Stable diagnostics service identity.
    pub const fn service(self) -> DiagnosticsServiceId {
        self.service
    }

    /// D-Bus scope on which the well-known name must be checked.
    pub const fn bus(self) -> ServiceBusScope {
        self.bus
    }

    /// Exact well-known D-Bus name.
    pub const fn bus_name(self) -> &'static str {
        self.bus_name
    }
}

/// Orbis Hardware1 on the system bus.
pub const ORBIS_HARDWARE_SERVICE: ServicePresenceTarget = ServicePresenceTarget {
    service: DiagnosticsServiceId::OrbisHardwared,
    bus: ServiceBusScope::System,
    bus_name: ORBIS_HARDWARE_BUS_NAME,
};
/// Orbis Session1 on the user/session bus.
pub const ORBIS_SESSION_SERVICE: ServicePresenceTarget = ServicePresenceTarget {
    service: DiagnosticsServiceId::OrbisSessiond,
    bus: ServiceBusScope::Session,
    bus_name: ORBIS_SESSION_BUS_NAME,
};
/// asusd on the system bus.
pub const ASUSD_SERVICE: ServicePresenceTarget = ServicePresenceTarget {
    service: DiagnosticsServiceId::Asusd,
    bus: ServiceBusScope::System,
    bus_name: ASUSD_BUS_NAME,
};
/// supergfxd on the system bus.
pub const SUPERGFXD_SERVICE: ServicePresenceTarget = ServicePresenceTarget {
    service: DiagnosticsServiceId::Supergfxd,
    bus: ServiceBusScope::System,
    bus_name: SUPERGFXD_BUS_NAME,
};

/// Read-only service-presence provider over already-open system/session buses.
///
/// Construction performs no I/O. Checks call only D-Bus daemon
/// `NameHasOwner` and, when needed, `ListActivatableNames`. They never call
/// `StartServiceByName`, so diagnostics cannot activate a stopped service.
#[derive(Clone)]
pub struct ServicePresenceProvider {
    system: Connection,
    session: Connection,
}

impl ServicePresenceProvider {
    /// Create a provider over prepared bus connections without performing I/O.
    pub fn new(system: Connection, session: Connection) -> Self {
        Self { system, session }
    }

    /// Observe one service without inferring capability support or criticality.
    pub async fn check(
        &self,
        target: ServicePresenceTarget,
        criticality: ServiceCriticality,
        checked_at: Option<SystemTime>,
    ) -> ServiceDiagnostics {
        let connection = match target.bus() {
            ServiceBusScope::System => &self.system,
            ServiceBusScope::Session => &self.session,
        };
        let query = ZbusPresenceQuery { connection };
        check_with_query(&query, target, criticality, checked_at).await
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PresenceQueryError {
    PermissionDenied,
    Unknown,
}

#[async_trait]
trait BusPresenceQuery: Send + Sync {
    async fn name_has_owner(&self, bus_name: &str) -> Result<bool, PresenceQueryError>;
    async fn list_activatable_names(&self) -> Result<Vec<String>, PresenceQueryError>;
}

struct ZbusPresenceQuery<'a> {
    connection: &'a Connection,
}

#[async_trait]
impl BusPresenceQuery for ZbusPresenceQuery<'_> {
    async fn name_has_owner(&self, bus_name: &str) -> Result<bool, PresenceQueryError> {
        let proxy = DBusProxy::new(self.connection)
            .await
            .map_err(map_zbus_error)?;
        let bus_name = BusName::try_from(bus_name).map_err(|_| PresenceQueryError::Unknown)?;
        proxy.name_has_owner(bus_name).await.map_err(map_fdo_error)
    }

    async fn list_activatable_names(&self) -> Result<Vec<String>, PresenceQueryError> {
        let proxy = DBusProxy::new(self.connection)
            .await
            .map_err(map_zbus_error)?;
        proxy
            .list_activatable_names()
            .await
            .map(|names| names.into_iter().map(|name| name.to_string()).collect())
            .map_err(map_fdo_error)
    }
}

fn map_zbus_error(error: zbus::Error) -> PresenceQueryError {
    match error {
        zbus::Error::FDO(error) => map_fdo_error(*error),
        _ => PresenceQueryError::Unknown,
    }
}

fn map_fdo_error(error: zbus::fdo::Error) -> PresenceQueryError {
    match error {
        zbus::fdo::Error::AccessDenied(_) => PresenceQueryError::PermissionDenied,
        _ => PresenceQueryError::Unknown,
    }
}

async fn check_with_query(
    query: &dyn BusPresenceQuery,
    target: ServicePresenceTarget,
    criticality: ServiceCriticality,
    checked_at: Option<SystemTime>,
) -> ServiceDiagnostics {
    let availability = match query.name_has_owner(target.bus_name()).await {
        Ok(true) => ServiceAvailability::Running,
        Ok(false) => match query.list_activatable_names().await {
            Ok(names) if names.iter().any(|name| name == target.bus_name()) => {
                ServiceAvailability::Activatable
            }
            Ok(_) => ServiceAvailability::Unavailable,
            Err(PresenceQueryError::PermissionDenied) => ServiceAvailability::PermissionDenied,
            Err(PresenceQueryError::Unknown) => ServiceAvailability::Unknown,
        },
        Err(PresenceQueryError::PermissionDenied) => ServiceAvailability::PermissionDenied,
        Err(PresenceQueryError::Unknown) => ServiceAvailability::Unknown,
    };

    ServiceDiagnostics {
        service: target.service(),
        bus: target.bus(),
        availability,
        criticality,
        checked_at,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;

    struct FakeQuery {
        owner: Result<bool, PresenceQueryError>,
        activatable: Result<Vec<String>, PresenceQueryError>,
        owner_names: Mutex<Vec<String>>,
        activatable_calls: AtomicUsize,
    }

    impl FakeQuery {
        fn new(
            owner: Result<bool, PresenceQueryError>,
            activatable: Result<Vec<String>, PresenceQueryError>,
        ) -> Self {
            Self {
                owner,
                activatable,
                owner_names: Mutex::new(Vec::new()),
                activatable_calls: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl BusPresenceQuery for FakeQuery {
        async fn name_has_owner(&self, bus_name: &str) -> Result<bool, PresenceQueryError> {
            self.owner_names.lock().unwrap().push(bus_name.to_owned());
            self.owner
        }

        async fn list_activatable_names(&self) -> Result<Vec<String>, PresenceQueryError> {
            self.activatable_calls.fetch_add(1, Ordering::SeqCst);
            self.activatable.clone()
        }
    }

    #[test]
    fn service_targets_have_exact_names_and_scopes() {
        let targets = [
            (
                ORBIS_HARDWARE_SERVICE,
                DiagnosticsServiceId::OrbisHardwared,
                ServiceBusScope::System,
                "io.github.orbiscontrol.Hardware",
            ),
            (
                ORBIS_SESSION_SERVICE,
                DiagnosticsServiceId::OrbisSessiond,
                ServiceBusScope::Session,
                "io.github.orbiscontrol.Session",
            ),
            (
                ASUSD_SERVICE,
                DiagnosticsServiceId::Asusd,
                ServiceBusScope::System,
                "xyz.ljones.Asusd",
            ),
            (
                SUPERGFXD_SERVICE,
                DiagnosticsServiceId::Supergfxd,
                ServiceBusScope::System,
                "org.supergfxctl.Daemon",
            ),
        ];

        for (target, service, bus, name) in targets {
            assert_eq!(target.service(), service);
            assert_eq!(target.bus(), bus);
            assert_eq!(target.bus_name(), name);
        }
    }

    #[tokio::test]
    async fn running_owner_skips_activatable_query() {
        let query = FakeQuery::new(Ok(true), Ok(Vec::new()));
        let diagnostics = check_with_query(
            &query,
            ORBIS_HARDWARE_SERVICE,
            ServiceCriticality::CapabilityLocal,
            None,
        )
        .await;
        assert_eq!(diagnostics.availability, ServiceAvailability::Running);
        assert_eq!(query.activatable_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn absent_owner_classifies_activatable_or_unavailable() {
        let activatable = FakeQuery::new(Ok(false), Ok(vec![ORBIS_SESSION_BUS_NAME.to_owned()]));
        let diagnostics = check_with_query(
            &activatable,
            ORBIS_SESSION_SERVICE,
            ServiceCriticality::CoreReadPath,
            None,
        )
        .await;
        assert_eq!(diagnostics.availability, ServiceAvailability::Activatable);

        let unavailable = FakeQuery::new(Ok(false), Ok(vec!["other.service".into()]));
        let diagnostics = check_with_query(
            &unavailable,
            ASUSD_SERVICE,
            ServiceCriticality::CapabilityLocal,
            None,
        )
        .await;
        assert_eq!(diagnostics.availability, ServiceAvailability::Unavailable);
    }

    #[tokio::test]
    async fn denied_queries_remain_permission_denied() {
        for query in [
            FakeQuery::new(Err(PresenceQueryError::PermissionDenied), Ok(Vec::new())),
            FakeQuery::new(Ok(false), Err(PresenceQueryError::PermissionDenied)),
        ] {
            let diagnostics = check_with_query(
                &query,
                SUPERGFXD_SERVICE,
                ServiceCriticality::Optional,
                None,
            )
            .await;
            assert_eq!(
                diagnostics.availability,
                ServiceAvailability::PermissionDenied
            );
        }
    }

    #[tokio::test]
    async fn other_query_failures_remain_unknown() {
        for query in [
            FakeQuery::new(Err(PresenceQueryError::Unknown), Ok(Vec::new())),
            FakeQuery::new(Ok(false), Err(PresenceQueryError::Unknown)),
        ] {
            let diagnostics = check_with_query(
                &query,
                ORBIS_HARDWARE_SERVICE,
                ServiceCriticality::CoreReadPath,
                None,
            )
            .await;
            assert_eq!(diagnostics.availability, ServiceAvailability::Unknown);
        }
    }

    #[tokio::test]
    async fn caller_owned_metadata_is_preserved_without_inference() {
        let checked_at = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(42);
        let query = FakeQuery::new(Ok(false), Ok(Vec::new()));
        let diagnostics = check_with_query(
            &query,
            SUPERGFXD_SERVICE,
            ServiceCriticality::Optional,
            Some(checked_at),
        )
        .await;

        assert_eq!(diagnostics.service, DiagnosticsServiceId::Supergfxd);
        assert_eq!(diagnostics.bus, ServiceBusScope::System);
        assert_eq!(diagnostics.criticality, ServiceCriticality::Optional);
        assert_eq!(diagnostics.checked_at, Some(checked_at));
        assert_eq!(
            query.owner_names.lock().unwrap().as_slice(),
            &[SUPERGFXD_BUS_NAME.to_owned()]
        );
    }

    #[test]
    fn only_fdo_access_denied_maps_to_permission_denied() {
        assert_eq!(
            map_fdo_error(zbus::fdo::Error::AccessDenied("denied".into())),
            PresenceQueryError::PermissionDenied
        );
        assert_eq!(
            map_fdo_error(zbus::fdo::Error::Failed("failed".into())),
            PresenceQueryError::Unknown
        );
    }
}
