//! Private P2P integration tests для production
//! `orbis_sessiond::discovery::discover_battery_object_path`.
//!
//! Используется fake UPower root (EnumerateDevices) и fake Device objects
//! (Type/PowerSupply); реальная system/session bus не задействована.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use orbis_providers::error::ProviderError;
use orbis_sessiond::discovery::discover_battery_object_path;
use zbus::connection::Builder;

/// Test-only UPower constants.
const UPOWER_BUS_NAME: &str = "org.freedesktop.UPower";
const UPOWER_ROOT_PATH: &str = "/org/freedesktop/UPower";

/// Property идентификатор для call log и fail_at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FakeProp {
    Type,
    PowerSupply,
}

/// Состояние fake Device.
struct FakeDeviceState {
    device_type: u32,
    power_supply: bool,
    fail_at: Option<FakeProp>,
    calls: Vec<FakeProp>,
}

impl FakeDeviceState {
    fn new(device_type: u32, power_supply: bool) -> Self {
        Self {
            device_type,
            power_supply,
            fail_at: None,
            calls: Vec::new(),
        }
    }
}

/// Test-only fake UPower Device.
struct FakeDevice {
    state: Arc<Mutex<FakeDeviceState>>,
}

#[zbus::interface(name = "org.freedesktop.UPower.Device")]
impl FakeDevice {
    /// Fake property: Type.
    #[zbus(property)]
    fn type_(&self) -> zbus::fdo::Result<u32> {
        let mut st = self.state.lock().unwrap();
        st.calls.push(FakeProp::Type);
        if st.fail_at == Some(FakeProp::Type) {
            return Err(zbus::fdo::Error::Failed(
                "scripted UPower discovery failure".to_owned(),
            ));
        }
        Ok(st.device_type)
    }

    /// Fake property: PowerSupply.
    #[zbus(property)]
    fn power_supply(&self) -> zbus::fdo::Result<bool> {
        let mut st = self.state.lock().unwrap();
        st.calls.push(FakeProp::PowerSupply);
        if st.fail_at == Some(FakeProp::PowerSupply) {
            return Err(zbus::fdo::Error::Failed(
                "scripted UPower discovery failure".to_owned(),
            ));
        }
        Ok(st.power_supply)
    }
}

/// Состояние fake UPower root.
struct FakeRootState {
    paths: Vec<zbus::zvariant::OwnedObjectPath>,
    calls: Vec<&'static str>,
}

/// Test-only fake UPower root.
struct FakeRoot {
    state: Arc<Mutex<FakeRootState>>,
}

#[zbus::interface(name = "org.freedesktop.UPower")]
impl FakeRoot {
    /// Fake method: EnumerateDevices.
    fn enumerate_devices(&self) -> zbus::fdo::Result<Vec<zbus::zvariant::OwnedObjectPath>> {
        let mut st = self.state.lock().unwrap();
        st.calls.push("EnumerateDevices");
        Ok(st.paths.clone())
    }
}

/// Поднять P2P pair: fake UPower root + fake devices на одной server connection.
async fn connect_discovery(
    root: Arc<Mutex<FakeRootState>>,
    devices: Vec<(zbus::zvariant::OwnedObjectPath, Arc<Mutex<FakeDeviceState>>)>,
) -> Result<(zbus::Connection, zbus::Connection), Box<dyn std::error::Error + Send + Sync>> {
    let (server_stream, client_stream) = std::os::unix::net::UnixStream::pair()?;
    let guid = zbus::Guid::generate();
    let mut server_builder = Builder::unix_stream(server_stream)
        .server(guid)?
        .p2p()
        .name(UPOWER_BUS_NAME)?
        .serve_at(UPOWER_ROOT_PATH, FakeRoot { state: root })?;
    for (path, state) in devices {
        server_builder = server_builder.serve_at(path, FakeDevice { state })?;
    }
    let client_builder = Builder::unix_stream(client_stream).p2p();
    let (server_conn, client_conn) =
        tokio::try_join!(server_builder.build(), client_builder.build())?;
    Ok((server_conn, client_conn))
}

fn obj_path(s: &str) -> zbus::zvariant::OwnedObjectPath {
    s.to_string().try_into().expect("valid object path")
}

#[tokio::test]
async fn discovers_single_system_battery() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let line_power = obj_path("/org/freedesktop/UPower/devices/line_power_AC");
        let peripheral = obj_path("/org/freedesktop/UPower/devices/mouse_hid");
        let system_battery = obj_path("/org/freedesktop/UPower/devices/battery_BAT1");

        let root = Arc::new(Mutex::new(FakeRootState {
            paths: vec![
                line_power.clone(),
                peripheral.clone(),
                system_battery.clone(),
            ],
            calls: Vec::new(),
        }));
        let lp_state = Arc::new(Mutex::new(FakeDeviceState::new(1, true)));
        let per_state = Arc::new(Mutex::new(FakeDeviceState::new(2, false)));
        let sys_state = Arc::new(Mutex::new(FakeDeviceState::new(2, true)));

        let (_server, client) = connect_discovery(
            root.clone(),
            vec![
                (line_power.clone(), lp_state.clone()),
                (peripheral.clone(), per_state.clone()),
                (system_battery.clone(), sys_state.clone()),
            ],
        )
        .await
        .expect("p2p connect");

        let found = discover_battery_object_path(&client)
            .await
            .expect("discover");
        assert_eq!(found, system_battery);

        // Порядок property calls.
        assert_eq!(root.lock().unwrap().calls, vec!["EnumerateDevices"]);
        assert_eq!(lp_state.lock().unwrap().calls, vec![FakeProp::Type]);
        assert_eq!(
            per_state.lock().unwrap().calls,
            vec![FakeProp::Type, FakeProp::PowerSupply]
        );
        assert_eq!(
            sys_state.lock().unwrap().calls,
            vec![FakeProp::Type, FakeProp::PowerSupply]
        );
    })
    .await
    .expect("p2p test timeout");
}

#[tokio::test]
async fn rejects_missing_system_battery() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let line_power = obj_path("/org/freedesktop/UPower/devices/line_power_AC");
        let peripheral = obj_path("/org/freedesktop/UPower/devices/mouse_hid");

        let root = Arc::new(Mutex::new(FakeRootState {
            paths: vec![line_power.clone(), peripheral.clone()],
            calls: Vec::new(),
        }));
        let lp_state = Arc::new(Mutex::new(FakeDeviceState::new(1, true)));
        let per_state = Arc::new(Mutex::new(FakeDeviceState::new(2, false)));

        let (_server, client) = connect_discovery(
            root.clone(),
            vec![(line_power, lp_state), (peripheral, per_state)],
        )
        .await
        .expect("p2p connect");

        let err = discover_battery_object_path(&client)
            .await
            .expect_err("missing");
        assert!(matches!(err, ProviderError::Unsupported(_)));
    })
    .await
    .expect("p2p test timeout");
}

#[tokio::test]
async fn rejects_multiple_system_batteries() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let battery_a = obj_path("/org/freedesktop/UPower/devices/battery_BAT0");
        let battery_b = obj_path("/org/freedesktop/UPower/devices/battery_BAT1");

        let root = Arc::new(Mutex::new(FakeRootState {
            paths: vec![battery_a.clone(), battery_b.clone()],
            calls: Vec::new(),
        }));
        let a_state = Arc::new(Mutex::new(FakeDeviceState::new(2, true)));
        let b_state = Arc::new(Mutex::new(FakeDeviceState::new(2, true)));

        let (_server, client) = connect_discovery(
            root.clone(),
            vec![(battery_a, a_state), (battery_b, b_state)],
        )
        .await
        .expect("p2p connect");

        let err = discover_battery_object_path(&client)
            .await
            .expect_err("multiple");
        // Первый кандидат не выбирается молча: ошибка, не Some(BAT0).
        assert!(matches!(err, ProviderError::Unsupported(_)));
    })
    .await
    .expect("p2p test timeout");
}

#[tokio::test]
async fn reads_fresh_device_state() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let battery = obj_path("/org/freedesktop/UPower/devices/battery_BAT1");
        let root = Arc::new(Mutex::new(FakeRootState {
            paths: vec![battery.clone()],
            calls: Vec::new(),
        }));
        let state = Arc::new(Mutex::new(FakeDeviceState::new(2, true)));

        let (_server, client) =
            connect_discovery(root.clone(), vec![(battery.clone(), state.clone())])
                .await
                .expect("p2p connect");

        let first = discover_battery_object_path(&client).await.expect("read1");
        assert_eq!(first, battery);

        {
            let mut st = state.lock().unwrap();
            st.power_supply = false;
        }

        let second = discover_battery_object_path(&client)
            .await
            .expect_err("read2");
        assert!(matches!(second, ProviderError::Unsupported(_)));

        // No-cache доказан: root и device перечитаны заново.
        assert_eq!(
            root.lock().unwrap().calls,
            vec!["EnumerateDevices", "EnumerateDevices"]
        );
        assert_eq!(
            state.lock().unwrap().calls,
            vec![
                FakeProp::Type,
                FakeProp::PowerSupply,
                FakeProp::Type,
                FakeProp::PowerSupply
            ]
        );
    })
    .await
    .expect("p2p test timeout");
}

#[tokio::test]
async fn maps_device_property_error_to_dbus() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let battery = obj_path("/org/freedesktop/UPower/devices/battery_BAT1");
        let root = Arc::new(Mutex::new(FakeRootState {
            paths: vec![battery.clone()],
            calls: Vec::new(),
        }));
        let state = Arc::new(Mutex::new(FakeDeviceState::new(2, true)));
        {
            let mut st = state.lock().unwrap();
            st.fail_at = Some(FakeProp::Type);
        }

        let (_server, client) = connect_discovery(root.clone(), vec![(battery, state.clone())])
            .await
            .expect("p2p connect");

        let err = discover_battery_object_path(&client)
            .await
            .expect_err("dbus");
        assert!(matches!(err, ProviderError::Dbus(_)));

        // Ошибка на Type: PowerSupply не читается.
        assert_eq!(state.lock().unwrap().calls, vec![FakeProp::Type]);
    })
    .await
    .expect("p2p test timeout");
}
