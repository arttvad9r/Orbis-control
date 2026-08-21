//! Private P2P integration tests production composition helper
//! (`orbis_sessiond::composition::build_upower_session_server`).
//!
//! Полный путь: fake UPower → production source → production provider →
//! production composition → production session server helper → production
//! SessionService → generated Session1Proxy. Используются две независимые
//! пары Unix streams (UPower и session); внешние daemon/system/session bus не
//! задействованы.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use orbis_providers::error::ProviderError;
use orbis_session_protocol::Session1Proxy;
use orbis_sessiond::composition::build_upower_session_server_with_effective_source;
use orbis_sessiond::upower::{AsusdConfiguredSource, BatteryEffectiveSource};
use zbus::connection::Builder;
use zbus::proxy::CacheProperties;

/// Test-only UPower constants.
const UPOWER_BUS_NAME: &str = "org.freedesktop.UPower";
const BATTERY_OBJECT_PATH: &str = "/org/freedesktop/UPower/devices/battery_BAT1";

struct FakeEffectiveSource;

#[async_trait]
impl BatteryEffectiveSource for FakeEffectiveSource {
    async fn read_effective_end_threshold(&self) -> Result<u8, ProviderError> {
        Ok(100)
    }
}

struct FakeAsusdSource(u8);

#[async_trait]
impl AsusdConfiguredSource for FakeAsusdSource {
    async fn read_configured_threshold(&self) -> Result<u8, ProviderError> {
        Ok(self.0)
    }
}

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
                "scripted UPower failure".to_owned(),
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
                "scripted UPower failure".to_owned(),
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
                "scripted UPower failure".to_owned(),
            ));
        }
        Ok(st.end_threshold)
    }
}

/// Поднять две независимые P2P пары (UPower + session) и полный composition.
///
/// Возвращает четыре Connection; proxy создаётся тестом из session client
/// connection (чтобы избежать self-referential lifetime).
async fn connect_composition(
    state: Arc<Mutex<FakeUPowerState>>,
) -> Result<
    (
        zbus::Connection,
        zbus::Connection,
        zbus::Connection,
        zbus::Connection,
    ),
    Box<dyn std::error::Error + Send + Sync>,
> {
    // UPower pair.
    let fake = FakeUPowerDevice {
        state: state.clone(),
    };
    let (upower_server_stream, upower_client_stream) = std::os::unix::net::UnixStream::pair()?;
    let upower_guid = zbus::Guid::generate();
    let upower_server_builder = Builder::unix_stream(upower_server_stream)
        .server(upower_guid)?
        .p2p()
        .name(UPOWER_BUS_NAME)?
        .serve_at(BATTERY_OBJECT_PATH, fake)?;
    let upower_client_builder = Builder::unix_stream(upower_client_stream).p2p();
    let (upower_server_conn, upower_client_conn) =
        tokio::try_join!(upower_server_builder.build(), upower_client_builder.build(),)?;

    // Session pair: server builder передаётся composition helper без name/serve_at.
    let (session_server_stream, session_client_stream) = std::os::unix::net::UnixStream::pair()?;
    let session_guid = zbus::Guid::generate();
    let session_server_builder = Builder::unix_stream(session_server_stream)
        .server(session_guid)?
        .p2p();
    let session_client_builder = Builder::unix_stream(session_client_stream).p2p();

    let object_path: zbus::zvariant::OwnedObjectPath = BATTERY_OBJECT_PATH
        .to_string()
        .try_into()
        .expect("valid object path");
    let (session_server_conn, session_client_conn) = tokio::try_join!(
        build_upower_session_server_with_effective_source(
            session_server_builder,
            upower_client_conn.clone(),
            object_path,
            FakeAsusdSource(100),
            FakeEffectiveSource,
            Default::default(),
            None,
            None,
        ),
        session_client_builder.build(),
    )?;

    Ok((
        upower_server_conn,
        upower_client_conn,
        session_server_conn,
        session_client_conn,
    ))
}

#[tokio::test]
async fn composition_serves_upower_charge_limit() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let state = Arc::new(Mutex::new(FakeUPowerState::new(true, false, 100)));
        let (_u_s, _u_c, _s_s, _s_c) = connect_composition(state.clone())
            .await
            .expect("p2p connect");
        let proxy = Session1Proxy::builder(&_s_c)
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .expect("proxy");

        let info = proxy.charge_limit().await.expect("charge limit");

        assert!(!info.enabled);
        assert!(info.configured_percent_present);
        assert_eq!(info.configured_percent, 100);
        assert!(info.effective_percent_present);
        assert_eq!(info.effective_percent, 100);
        // UPower не сообщает hardware bounds: на wire bounds отсутствуют.
        assert!(!info.bounds_present);
        assert_eq!(info.min_percent, 0);
        assert_eq!(info.max_percent, 0);
        assert_eq!(info.step_percent, 0);

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
async fn composition_reads_fresh_upower_values() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let state = Arc::new(Mutex::new(FakeUPowerState::new(true, true, 100)));
        let (_u_s, _u_c, _s_s, _s_c) = connect_composition(state.clone())
            .await
            .expect("p2p connect");
        let proxy = Session1Proxy::builder(&_s_c)
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .expect("proxy");

        let first = proxy.charge_limit().await.expect("read1");
        assert!(first.enabled);
        assert_eq!(first.configured_percent, 100);
        assert_eq!(first.effective_percent, 100);

        {
            let mut st = state.lock().unwrap();
            st.enabled = false;
            st.end_threshold = 60;
        }

        let second = proxy.charge_limit().await;
        assert!(matches!(second, Err(zbus::Error::FDO(_))));

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
async fn composition_maps_upower_failure_to_session_failed() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let state = Arc::new(Mutex::new(FakeUPowerState::new(true, true, 80)));
        {
            let mut st = state.lock().unwrap();
            st.fail_at = Some(FakeProperty::Enabled);
        }
        let (_u_s, _u_c, _s_s, _s_c) = connect_composition(state.clone())
            .await
            .expect("p2p connect");
        let proxy = Session1Proxy::builder(&_s_c)
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .expect("proxy");

        let err = proxy.charge_limit().await.expect_err("session failed");

        match err {
            zbus::Error::FDO(boxed) => match &*boxed {
                zbus::fdo::Error::Failed(msg) => {
                    assert!(msg.contains("scripted UPower failure"));
                }
                other => panic!("ожидался FDO(Failed), получено: {other:?}"),
            },
            other => panic!("ожидался FDO, получено: {other:?}"),
        }

        let calls = state.lock().unwrap().calls.clone();
        assert_eq!(calls, vec![FakeProperty::Supported, FakeProperty::Enabled]);
    })
    .await
    .expect("p2p test timeout");
}

#[tokio::test]
async fn composition_maps_unsupported_device_to_not_supported() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let state = Arc::new(Mutex::new(FakeUPowerState::new(false, false, 80)));
        let (_u_s, _u_c, _s_s, _s_c) = connect_composition(state.clone())
            .await
            .expect("p2p connect");
        let proxy = Session1Proxy::builder(&_s_c)
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .expect("proxy");

        let err = proxy.charge_limit().await.expect_err("not supported");

        match err {
            zbus::Error::FDO(boxed) => {
                assert!(matches!(&*boxed, zbus::fdo::Error::NotSupported(_)));
            }
            other => panic!("ожидался FDO(NotSupported), получено: {other:?}"),
        }

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
