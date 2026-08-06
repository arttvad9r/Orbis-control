//! Private P2P integration tests для production
//! `ZbusUPowerChargeLimitSource`: настоящий D-Bus round-trip через test-only
//! fake `org.freedesktop.UPower.Device`.
//!
//! Проверяется фактическая совместимость generated proxy с UPower property ABI,
//! default service, переданный object path, строгий последовательный порядок
//! прямых property Get (CacheProperties::No, без GetAll), отсутствие кэша и
//! short-circuit после ошибки.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use orbis_providers::error::ProviderError;
use orbis_sessiond::upower::{UPowerChargeLimitSource, ZbusUPowerChargeLimitSource};
use zbus::connection::Builder;

/// Test-only UPower constants (production-код не изменяется).
const UPOWER_BUS_NAME: &str = "org.freedesktop.UPower";
const BATTERY_OBJECT_PATH: &str = "/org/freedesktop/UPower/devices/battery_BAT1";

/// Идентификатор property для call log и fail_at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FakeProperty {
    Supported,
    Enabled,
    EndThreshold,
}

/// Общее test-only состояние fake UPower device.
struct FakeUPowerState {
    supported: bool,
    enabled: bool,
    end_threshold: u32,
    fail_at: Option<FakeProperty>,
    calls: Vec<FakeProperty>,
}

impl FakeUPowerState {
    fn new(supported: bool, enabled: bool, end_threshold: u32) -> Self {
        Self {
            supported,
            enabled,
            end_threshold,
            fail_at: None,
            calls: Vec::new(),
        }
    }
}

/// Test-only fake UPower Device.
struct FakeUPowerDevice {
    state: Arc<Mutex<FakeUPowerState>>,
}

#[zbus::interface(name = "org.freedesktop.UPower.Device")]
impl FakeUPowerDevice {
    /// Fake property: ChargeThresholdSupported.
    #[zbus(property)]
    fn charge_threshold_supported(&self) -> zbus::fdo::Result<bool> {
        let mut st = self.state.lock().unwrap();
        st.calls.push(FakeProperty::Supported);
        if st.fail_at == Some(FakeProperty::Supported) {
            return Err(zbus::fdo::Error::Failed(
                "scripted UPower property failure".to_owned(),
            ));
        }
        Ok(st.supported)
    }

    /// Fake property: ChargeThresholdEnabled.
    #[zbus(property)]
    fn charge_threshold_enabled(&self) -> zbus::fdo::Result<bool> {
        let mut st = self.state.lock().unwrap();
        st.calls.push(FakeProperty::Enabled);
        if st.fail_at == Some(FakeProperty::Enabled) {
            return Err(zbus::fdo::Error::Failed(
                "scripted UPower property failure".to_owned(),
            ));
        }
        Ok(st.enabled)
    }

    /// Fake property: ChargeEndThreshold.
    #[zbus(property)]
    fn charge_end_threshold(&self) -> zbus::fdo::Result<u32> {
        let mut st = self.state.lock().unwrap();
        st.calls.push(FakeProperty::EndThreshold);
        if st.fail_at == Some(FakeProperty::EndThreshold) {
            return Err(zbus::fdo::Error::Failed(
                "scripted UPower property failure".to_owned(),
            ));
        }
        Ok(st.end_threshold)
    }
}

/// Поднять P2P pair: fake UPower server + production source.
async fn connect_upower(
    state: Arc<Mutex<FakeUPowerState>>,
) -> Result<
    (
        zbus::Connection,
        zbus::Connection,
        ZbusUPowerChargeLimitSource,
    ),
    Box<dyn std::error::Error + Send + Sync>,
> {
    let fake = FakeUPowerDevice {
        state: state.clone(),
    };
    let (server_stream, client_stream) = std::os::unix::net::UnixStream::pair()?;
    let guid = zbus::Guid::generate();
    let server_builder = Builder::unix_stream(server_stream)
        .server(guid)?
        .p2p()
        .name(UPOWER_BUS_NAME)?
        .serve_at(BATTERY_OBJECT_PATH, fake)?;
    let client_builder = Builder::unix_stream(client_stream).p2p();
    let (server_conn, client_conn) =
        tokio::try_join!(server_builder.build(), client_builder.build())?;

    let object_path: zbus::zvariant::OwnedObjectPath = BATTERY_OBJECT_PATH
        .to_string()
        .try_into()
        .expect("valid object path");
    let source = ZbusUPowerChargeLimitSource::new(client_conn.clone(), object_path);

    Ok((server_conn, client_conn, source))
}

#[tokio::test]
async fn upower_source_reads_snapshot_over_p2p() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let state = Arc::new(Mutex::new(FakeUPowerState::new(true, false, 80)));
        let (_server_conn, _client_conn, source) =
            connect_upower(state.clone()).await.expect("p2p connect");

        let snapshot = source.read_charge_limit().await.expect("snapshot");

        assert!(snapshot.supported);
        assert!(!snapshot.enabled);
        assert_eq!(snapshot.end_threshold, 80);

        let calls = state.lock().unwrap().calls.clone();
        assert_eq!(
            calls,
            vec![
                FakeProperty::Supported,
                FakeProperty::Enabled,
                FakeProperty::EndThreshold,
            ]
        );
    })
    .await
    .expect("p2p test timeout");
}

#[tokio::test]
async fn upower_source_reads_fresh_snapshot() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let state = Arc::new(Mutex::new(FakeUPowerState::new(true, true, 80)));
        let (_server_conn, _client_conn, source) =
            connect_upower(state.clone()).await.expect("p2p connect");

        let first = source.read_charge_limit().await.expect("read1");
        assert!(first.supported);
        assert!(first.enabled);
        assert_eq!(first.end_threshold, 80);

        {
            let mut st = state.lock().unwrap();
            st.enabled = false;
            st.end_threshold = 60;
        }

        let second = source.read_charge_limit().await.expect("read2");
        assert!(second.supported);
        assert!(!second.enabled);
        assert_eq!(second.end_threshold, 60);

        let calls = state.lock().unwrap().calls.clone();
        assert_eq!(
            calls,
            vec![
                FakeProperty::Supported,
                FakeProperty::Enabled,
                FakeProperty::EndThreshold,
                FakeProperty::Supported,
                FakeProperty::Enabled,
                FakeProperty::EndThreshold,
            ]
        );
    })
    .await
    .expect("p2p test timeout");
}

#[tokio::test]
async fn upower_source_maps_middle_property_error_to_dbus() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let state = Arc::new(Mutex::new(FakeUPowerState::new(true, true, 80)));
        {
            let mut st = state.lock().unwrap();
            st.fail_at = Some(FakeProperty::Enabled);
        }
        let (_server_conn, _client_conn, source) =
            connect_upower(state.clone()).await.expect("p2p connect");

        let err = source.read_charge_limit().await.expect_err("dbus error");

        match &err {
            ProviderError::Dbus(msg) => {
                assert!(
                    msg.contains("scripted UPower property failure"),
                    "ожидался test marker в ошибке, получено: {msg}"
                );
            }
            other => panic!("ожидался Dbus, получено: {other:?}"),
        }

        let calls = state.lock().unwrap().calls.clone();
        assert_eq!(calls, vec![FakeProperty::Supported, FakeProperty::Enabled]);
    })
    .await
    .expect("p2p test timeout");
}

#[tokio::test]
async fn upower_source_stops_after_first_property_error() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let state = Arc::new(Mutex::new(FakeUPowerState::new(true, true, 80)));
        {
            let mut st = state.lock().unwrap();
            st.fail_at = Some(FakeProperty::Supported);
        }
        let (_server_conn, _client_conn, source) =
            connect_upower(state.clone()).await.expect("p2p connect");

        let err = source.read_charge_limit().await.expect_err("dbus error");
        assert!(matches!(err, ProviderError::Dbus(_)));

        let calls = state.lock().unwrap().calls.clone();
        assert_eq!(calls, vec![FakeProperty::Supported]);
    })
    .await
    .expect("p2p test timeout");
}
