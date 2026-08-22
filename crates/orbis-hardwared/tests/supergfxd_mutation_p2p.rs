use std::sync::{Arc, Mutex};
use std::time::Duration;

use orbis_core::gpu::GpuPowerState;
use orbis_hardwared::supergfxd::{
    MutationObservation, SupergfxdMutationBackend, ZbusSupergfxdMutationClient,
};
use orbis_providers::ProviderError;
use orbis_providers::supergfxd::{SupergfxdMode, SupergfxdStagedState, SupergfxdUserAction};
use zbus::{connection::Builder, interface};

const OBJECT_PATH: &str = "/org/supergfxctl/Gfx";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FakeCall {
    Supported,
    SetMode(u32),
    Mode,
    PendingMode,
    PendingUserAction,
    Power,
}

#[derive(Clone)]
struct FakeState {
    supported: Vec<u32>,
    snapshot: FakeSnapshot,
    returned_action: u32,
    set_mode_error: bool,
    read_error: Option<String>,
    calls: Arc<Mutex<Vec<FakeCall>>>,
}

#[derive(Clone, Copy)]
struct FakeSnapshot {
    mode: u32,
    pending_mode: u32,
    action: u32,
    power: u32,
}

struct FakeSupergfxd {
    state: Arc<Mutex<FakeState>>,
}

fn fake_error(state: &FakeState) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(
        state
            .read_error
            .clone()
            .unwrap_or_else(|| "scripted fake failure".into()),
    )
}

#[interface(name = "org.supergfxctl.Daemon")]
impl FakeSupergfxd {
    async fn supported(&self) -> zbus::fdo::Result<Vec<u32>> {
        let state = self.state.lock().unwrap();
        state.calls.lock().unwrap().push(FakeCall::Supported);
        Ok(state.supported.clone())
    }

    async fn set_mode(&self, mode: u32) -> zbus::fdo::Result<u32> {
        let state = self.state.lock().unwrap();
        state.calls.lock().unwrap().push(FakeCall::SetMode(mode));
        if state.set_mode_error {
            return Err(fake_error(&state));
        }
        Ok(state.returned_action)
    }

    async fn mode(&self) -> zbus::fdo::Result<u32> {
        let state = self.state.lock().unwrap();
        state.calls.lock().unwrap().push(FakeCall::Mode);
        if state.read_error.is_some() {
            return Err(fake_error(&state));
        }
        Ok(state.snapshot.mode)
    }

    async fn pending_mode(&self) -> zbus::fdo::Result<u32> {
        let state = self.state.lock().unwrap();
        state.calls.lock().unwrap().push(FakeCall::PendingMode);
        if state.read_error.is_some() {
            return Err(fake_error(&state));
        }
        Ok(state.snapshot.pending_mode)
    }

    async fn pending_user_action(&self) -> zbus::fdo::Result<u32> {
        let state = self.state.lock().unwrap();
        state
            .calls
            .lock()
            .unwrap()
            .push(FakeCall::PendingUserAction);
        if state.read_error.is_some() {
            return Err(fake_error(&state));
        }
        Ok(state.snapshot.action)
    }

    async fn power(&self) -> zbus::fdo::Result<u32> {
        let state = self.state.lock().unwrap();
        state.calls.lock().unwrap().push(FakeCall::Power);
        if state.read_error.is_some() {
            return Err(fake_error(&state));
        }
        Ok(state.snapshot.power)
    }
}

async fn connect_fake(
    state: Arc<Mutex<FakeState>>,
) -> Result<(zbus::Connection, zbus::Connection), Box<dyn std::error::Error + Send + Sync>> {
    let (server_stream, client_stream) = std::os::unix::net::UnixStream::pair()?;
    let guid = zbus::Guid::generate();
    let server_builder = Builder::unix_stream(server_stream)
        .server(guid)?
        .p2p()
        .serve_at(OBJECT_PATH, FakeSupergfxd { state })?;
    let client_builder = Builder::unix_stream(client_stream).p2p();
    Ok(tokio::try_join!(
        server_builder.build(),
        client_builder.build()
    )?)
}

fn state(snapshot: FakeSnapshot, returned_action: u32) -> FakeState {
    FakeState {
        supported: vec![0, 1, 5],
        snapshot,
        returned_action,
        set_mode_error: false,
        read_error: None,
        calls: Arc::new(Mutex::new(Vec::new())),
    }
}

async fn operation(
    state: FakeState,
    requested: SupergfxdMode,
) -> (
    Arc<Mutex<FakeState>>,
    Result<MutationObservation, ProviderError>,
) {
    let state = Arc::new(Mutex::new(state));
    let (_server, client) = connect_fake(state.clone()).await.expect("private P2P");
    let backend = SupergfxdMutationBackend::new(ZbusSupergfxdMutationClient::new(client));
    let result = backend.request_mode(requested).await;
    (state, result)
}

fn snapshot(mode: u32, pending_mode: u32, action: u32) -> FakeSnapshot {
    FakeSnapshot {
        mode,
        pending_mode,
        action,
        power: 1,
    }
}

fn calls(state: &Arc<Mutex<FakeState>>) -> Vec<FakeCall> {
    state.lock().unwrap().calls.lock().unwrap().clone()
}

#[tokio::test]
async fn read_snapshot_reads_all_fields_once_in_order() {
    let state = Arc::new(Mutex::new(state(snapshot(0, 6, 4), 4)));
    let (_server, client) = connect_fake(state.clone()).await.expect("private P2P");
    let backend = SupergfxdMutationBackend::new(ZbusSupergfxdMutationClient::new(client));

    let result = backend.read_snapshot().await.expect("snapshot");
    assert_eq!(result.current_mode, SupergfxdMode::Hybrid);
    assert_eq!(result.pending_mode, SupergfxdMode::None);
    assert_eq!(result.pending_user_action, SupergfxdUserAction::Nothing);
    assert_eq!(result.power, GpuPowerState::Suspended);
    assert_eq!(
        calls(&state),
        vec![
            FakeCall::Mode,
            FakeCall::PendingMode,
            FakeCall::PendingUserAction,
            FakeCall::Power,
            FakeCall::Supported,
        ]
    );
}

#[tokio::test]
async fn unsupported_is_rejected_before_set_mode() {
    let (state, result) = operation(state(snapshot(0, 6, 4), 4), SupergfxdMode::AsusEgpu).await;
    assert!(matches!(result, Err(ProviderError::Unsupported(_))));
    assert_eq!(calls(&state), vec![FakeCall::Supported]);
}

#[tokio::test]
async fn immediate_applied_is_classified_from_fresh_snapshot() {
    let (state, result) = operation(state(snapshot(1, 6, 4), 4), SupergfxdMode::Integrated).await;
    assert_eq!(result.unwrap().state, SupergfxdStagedState::Applied);
    assert_eq!(
        calls(&state),
        vec![
            FakeCall::Supported,
            FakeCall::SetMode(1),
            FakeCall::Mode,
            FakeCall::PendingMode,
            FakeCall::PendingUserAction,
            FakeCall::Power,
            FakeCall::Supported,
        ]
    );
}

#[tokio::test]
async fn logout_is_staged() {
    let (state, result) = operation(state(snapshot(0, 1, 0), 0), SupergfxdMode::Integrated).await;
    assert_eq!(
        result.unwrap().state,
        SupergfxdStagedState::RequiresUserAction(SupergfxdUserAction::Logout)
    );
    assert_eq!(
        calls(&state),
        vec![
            FakeCall::Supported,
            FakeCall::SetMode(1),
            FakeCall::Mode,
            FakeCall::PendingMode,
            FakeCall::PendingUserAction,
            FakeCall::Power,
            FakeCall::Supported,
        ]
    );
}

#[tokio::test]
async fn nothing_return_does_not_hide_pending_state() {
    let (state, result) = operation(state(snapshot(1, 0, 4), 4), SupergfxdMode::Hybrid).await;
    assert_eq!(result.unwrap().state, SupergfxdStagedState::Pending);
    assert_eq!(
        calls(&state),
        vec![
            FakeCall::Supported,
            FakeCall::SetMode(0),
            FakeCall::Mode,
            FakeCall::PendingMode,
            FakeCall::PendingUserAction,
            FakeCall::Power,
            FakeCall::Supported,
        ]
    );
}

#[tokio::test]
async fn reboot_is_staged() {
    let (state, result) = operation(state(snapshot(0, 5, 1), 1), SupergfxdMode::AsusMuxDgpu).await;
    assert_eq!(
        result.unwrap().state,
        SupergfxdStagedState::RequiresUserAction(SupergfxdUserAction::Reboot)
    );
    assert_eq!(
        calls(&state),
        vec![
            FakeCall::Supported,
            FakeCall::SetMode(5),
            FakeCall::Mode,
            FakeCall::PendingMode,
            FakeCall::PendingUserAction,
            FakeCall::Power,
            FakeCall::Supported,
        ]
    );
}

#[tokio::test]
async fn contradictory_readback_is_inconsistent() {
    let (state, result) = operation(state(snapshot(0, 5, 4), 4), SupergfxdMode::Integrated).await;
    assert_eq!(result.unwrap().state, SupergfxdStagedState::Inconsistent);
    assert_eq!(
        calls(&state),
        vec![
            FakeCall::Supported,
            FakeCall::SetMode(1),
            FakeCall::Mode,
            FakeCall::PendingMode,
            FakeCall::PendingUserAction,
            FakeCall::Power,
            FakeCall::Supported,
        ]
    );
}

#[tokio::test]
async fn action_without_pending_is_inconsistent() {
    let (state, result) = operation(state(snapshot(0, 6, 0), 0), SupergfxdMode::Integrated).await;
    assert_eq!(result.unwrap().state, SupergfxdStagedState::Inconsistent);
    assert_eq!(
        calls(&state),
        vec![
            FakeCall::Supported,
            FakeCall::SetMode(1),
            FakeCall::Mode,
            FakeCall::PendingMode,
            FakeCall::PendingUserAction,
            FakeCall::Power,
            FakeCall::Supported,
        ]
    );
}

#[tokio::test]
async fn set_mode_failure_is_not_retried() {
    let mut fake = state(snapshot(0, 1, 0), 0);
    fake.set_mode_error = true;
    let (state, result) = operation(fake, SupergfxdMode::Integrated).await;
    assert!(result.is_err());
    assert_eq!(
        calls(&state),
        vec![FakeCall::Supported, FakeCall::SetMode(1)]
    );
}

#[tokio::test]
async fn readback_failure_is_not_retried() {
    let mut fake = state(snapshot(0, 1, 0), 0);
    fake.read_error = Some("read-back failed".into());
    let (state, result) = operation(fake, SupergfxdMode::Integrated).await;
    assert!(result.is_err());
    assert_eq!(
        calls(&state),
        vec![FakeCall::Supported, FakeCall::SetMode(1), FakeCall::Mode]
    );
}

#[tokio::test]
async fn future_wire_values_are_not_applied() {
    let mut fake = state(snapshot(99, 6, 99), 4);
    fake.supported = vec![0, 1, 99];
    let (state, result) = operation(fake, SupergfxdMode::Integrated).await;
    assert_eq!(result.unwrap().state, SupergfxdStagedState::Inconsistent);
    assert_eq!(
        calls(&state),
        vec![
            FakeCall::Supported,
            FakeCall::SetMode(1),
            FakeCall::Mode,
            FakeCall::PendingMode,
            FakeCall::PendingUserAction,
            FakeCall::Power,
            FakeCall::Supported,
        ]
    );
}

#[tokio::test]
async fn future_requested_mode_is_rejected_without_set_mode() {
    let (state, result) = operation(state(snapshot(0, 6, 4), 4), SupergfxdMode::Unknown(99)).await;
    assert!(matches!(result, Err(ProviderError::InvalidRequest(_))));
    assert!(calls(&state).is_empty());
}

#[tokio::test]
async fn p2p_test_is_bounded() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let (_state, result) =
            operation(state(snapshot(1, 6, 4), 4), SupergfxdMode::Integrated).await;
        assert!(result.is_ok());
    })
    .await
    .expect("bounded P2P test");
}

#[tokio::test]
async fn returned_action_contradiction_is_inconsistent() {
    let (state, result) = operation(state(snapshot(0, 1, 0), 4), SupergfxdMode::Integrated).await;
    assert_eq!(result.unwrap().state, SupergfxdStagedState::Inconsistent);
    assert_eq!(
        calls(&state),
        vec![
            FakeCall::Supported,
            FakeCall::SetMode(1),
            FakeCall::Mode,
            FakeCall::PendingMode,
            FakeCall::PendingUserAction,
            FakeCall::Power,
            FakeCall::Supported,
        ]
    );
}
