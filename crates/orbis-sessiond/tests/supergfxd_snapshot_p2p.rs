//! Private P2P tests for the read-only supergfxd staged contract.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use orbis_core::gpu::GpuPowerState;
use orbis_sessiond::supergfxd::{
    SupergfxdGpuSnapshotSource, SupergfxdMode, SupergfxdStagedState, SupergfxdUserAction,
    ZbusSupergfxdGpuPowerSource, classify_supergfxd_state,
};
use zbus::{connection::Builder, interface};

const OBJECT_PATH: &str = "/org/supergfxctl/Gfx";

#[derive(Clone)]
struct FakeState {
    mode: u32,
    pending_mode: u32,
    action: u32,
    power: u32,
    supported: Vec<u32>,
}

struct FakeSupergfxd {
    state: Arc<Mutex<FakeState>>,
}

#[interface(name = "org.supergfxctl.Daemon")]
impl FakeSupergfxd {
    async fn mode(&self) -> u32 {
        self.state.lock().unwrap().mode
    }

    async fn pending_mode(&self) -> u32 {
        self.state.lock().unwrap().pending_mode
    }

    async fn pending_user_action(&self) -> u32 {
        self.state.lock().unwrap().action
    }

    async fn power(&self) -> u32 {
        self.state.lock().unwrap().power
    }

    async fn supported(&self) -> Vec<u32> {
        self.state.lock().unwrap().supported.clone()
    }
}

async fn connect_fake(
    state: Arc<Mutex<FakeState>>,
) -> Result<
    (
        zbus::Connection,
        zbus::Connection,
        ZbusSupergfxdGpuPowerSource,
    ),
    Box<dyn std::error::Error + Send + Sync>,
> {
    let (server_stream, client_stream) = std::os::unix::net::UnixStream::pair()?;
    let guid = zbus::Guid::generate();
    let server_builder = Builder::unix_stream(server_stream)
        .server(guid)?
        .p2p()
        .serve_at(OBJECT_PATH, FakeSupergfxd { state })?;
    let client_builder = Builder::unix_stream(client_stream).p2p();
    let (server_conn, client_conn) =
        tokio::try_join!(server_builder.build(), client_builder.build())?;
    Ok((
        server_conn,
        client_conn.clone(),
        ZbusSupergfxdGpuPowerSource::new(client_conn),
    ))
}

fn state(mode: u32, pending_mode: u32, action: u32) -> FakeState {
    FakeState {
        mode,
        pending_mode,
        action,
        power: 1,
        supported: vec![0, 1, 5],
    }
}

#[tokio::test]
async fn staged_snapshot_contract_roundtrips_over_private_p2p() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let state = Arc::new(Mutex::new(state(1, 6, 4)));
        let (_server, _client, source) = connect_fake(state.clone()).await.expect("p2p");

        let snapshot = source.read_snapshot().await.expect("snapshot");
        assert_eq!(snapshot.current_mode, SupergfxdMode::Integrated);
        assert_eq!(snapshot.pending_mode, SupergfxdMode::None);
        assert_eq!(snapshot.pending_user_action, SupergfxdUserAction::Nothing);
        assert_eq!(snapshot.power, GpuPowerState::Suspended);
        assert_eq!(
            snapshot.supported_modes,
            vec![
                SupergfxdMode::Hybrid,
                SupergfxdMode::Integrated,
                SupergfxdMode::AsusMuxDgpu,
            ]
        );
        assert_eq!(
            classify_supergfxd_state(SupergfxdMode::Integrated, &snapshot),
            SupergfxdStagedState::Applied
        );

        state.lock().unwrap().mode = 0;
        state.lock().unwrap().pending_mode = 1;
        state.lock().unwrap().action = 0;
        let changed = source.read_snapshot().await.expect("fresh snapshot");
        assert_eq!(changed.current_mode, SupergfxdMode::Hybrid);
        assert_eq!(changed.pending_mode, SupergfxdMode::Integrated);
        assert_eq!(changed.pending_user_action, SupergfxdUserAction::Logout);
        assert_eq!(
            classify_supergfxd_state(SupergfxdMode::Integrated, &changed),
            SupergfxdStagedState::RequiresUserAction(SupergfxdUserAction::Logout)
        );
    })
    .await
    .expect("bounded P2P test");
}
