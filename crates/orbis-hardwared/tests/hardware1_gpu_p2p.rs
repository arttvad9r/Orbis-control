use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use async_trait::async_trait;
use orbis_core::gpu::GpuPowerState;
use orbis_hardwared::{
    AuthorizeError, Authorizer, Hardware1Proxy, HardwareService, handle_set_gpu_mode,
    supergfxd::{MutationObservation, SupergfxdMutationOperation},
};
use orbis_providers::ProviderError;
use orbis_providers::supergfxd::{
    SupergfxdMode, SupergfxdSnapshot, SupergfxdStagedState, SupergfxdUserAction,
};
use zbus::connection::Builder;

const OBJECT_PATH: &str = "/io/github/orbiscontrol/Hardware";
const EXPECTED_SENDER: &str = ":1.77";

#[derive(Clone, Copy)]
enum AuthResult {
    Allow,
    Deny,
    Fail,
}

#[derive(Clone)]
struct FakeAuthorizer {
    result: AuthResult,
    calls: Arc<AtomicUsize>,
    sender: Arc<Mutex<Option<String>>>,
}

impl FakeAuthorizer {
    fn new(result: AuthResult) -> Self {
        Self {
            result,
            calls: Arc::new(AtomicUsize::new(0)),
            sender: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait]
impl Authorizer for FakeAuthorizer {
    async fn authorize(&self, sender: &str) -> Result<(), AuthorizeError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.sender.lock().unwrap() = Some(sender.to_owned());
        match self.result {
            AuthResult::Allow => Ok(()),
            AuthResult::Deny => Err(AuthorizeError::Denied("denied".into())),
            AuthResult::Fail => Err(AuthorizeError::Failed(
                "authorization backend failed".into(),
            )),
        }
    }
}

#[derive(Clone)]
struct FakeGpuBackend {
    result: Arc<Mutex<Result<MutationObservation, ProviderError>>>,
    calls: Arc<AtomicUsize>,
    requested: Arc<Mutex<Option<SupergfxdMode>>>,
}

impl FakeGpuBackend {
    fn new(result: Result<MutationObservation, ProviderError>) -> Self {
        Self {
            result: Arc::new(Mutex::new(result)),
            calls: Arc::new(AtomicUsize::new(0)),
            requested: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait]
impl SupergfxdMutationOperation for FakeGpuBackend {
    async fn request_mode(
        &self,
        requested: SupergfxdMode,
    ) -> Result<MutationObservation, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.requested.lock().unwrap() = Some(requested);
        match &*self.result.lock().unwrap() {
            Ok(observation) => Ok(observation.clone()),
            Err(error) => Err(ProviderError::Internal(error.to_string())),
        }
    }
}

fn observation(state: SupergfxdStagedState, action: SupergfxdUserAction) -> MutationObservation {
    MutationObservation {
        requested: SupergfxdMode::Integrated,
        returned_action: action,
        snapshot: SupergfxdSnapshot {
            current_mode: SupergfxdMode::Hybrid,
            pending_mode: SupergfxdMode::Integrated,
            pending_user_action: action,
            power: GpuPowerState::Suspended,
            supported_modes: vec![SupergfxdMode::Hybrid, SupergfxdMode::Integrated],
        },
        state,
    }
}

async fn connect_service(
    service: HardwareService,
) -> Result<(zbus::Connection, zbus::Connection), Box<dyn std::error::Error + Send + Sync>> {
    let (server_stream, client_stream) = std::os::unix::net::UnixStream::pair()?;
    let server_builder = Builder::unix_stream(server_stream)
        .server(zbus::Guid::generate())?
        .p2p()
        .serve_at(OBJECT_PATH, service)?;
    let client_builder = Builder::unix_stream(client_stream).p2p();
    Ok(tokio::try_join!(
        server_builder.build(),
        client_builder.build()
    )?)
}

fn proxy(
    connection: &zbus::Connection,
) -> impl std::future::Future<Output = zbus::Result<Hardware1Proxy<'_>>> {
    Hardware1Proxy::builder(connection).build()
}

#[tokio::test]
async fn invalid_wire_has_zero_auth_and_backend_calls() {
    let auth = FakeAuthorizer::new(AuthResult::Allow);
    let backend = FakeGpuBackend::new(Ok(observation(
        SupergfxdStagedState::Applied,
        SupergfxdUserAction::Nothing,
    )));
    let err = handle_set_gpu_mode(&auth, Some(&backend), 99, EXPECTED_SENDER)
        .await
        .expect_err("invalid wire");
    assert!(matches!(err, zbus::fdo::Error::InvalidArgs(_)));
    assert_eq!(auth.calls.load(Ordering::SeqCst), 0);
    assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn denied_and_auth_failure_do_not_call_backend() {
    for result in [AuthResult::Deny, AuthResult::Fail] {
        let auth = FakeAuthorizer::new(result);
        let backend = FakeGpuBackend::new(Ok(observation(
            SupergfxdStagedState::Applied,
            SupergfxdUserAction::Nothing,
        )));
        let err = handle_set_gpu_mode(&auth, Some(&backend), 1, EXPECTED_SENDER)
            .await
            .expect_err("authorization failure");
        assert!(matches!(
            (result, err),
            (AuthResult::Deny, zbus::fdo::Error::AccessDenied(_))
                | (AuthResult::Fail, zbus::fdo::Error::Failed(_))
        ));
        assert_eq!(auth.calls.load(Ordering::SeqCst), 1);
        assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn p2p_hardware1_returns_all_staged_observations() {
    for (state, action, expected) in [
        (
            SupergfxdStagedState::Applied,
            SupergfxdUserAction::Nothing,
            0,
        ),
        (
            SupergfxdStagedState::Pending,
            SupergfxdUserAction::Nothing,
            1,
        ),
        (
            SupergfxdStagedState::RequiresUserAction(SupergfxdUserAction::Logout),
            SupergfxdUserAction::Logout,
            2,
        ),
        (
            SupergfxdStagedState::RequiresUserAction(SupergfxdUserAction::Reboot),
            SupergfxdUserAction::Reboot,
            2,
        ),
        (
            SupergfxdStagedState::Inconsistent,
            SupergfxdUserAction::Nothing,
            3,
        ),
    ] {
        let auth = FakeAuthorizer::new(AuthResult::Allow);
        let backend = FakeGpuBackend::new(Ok(observation(state, action)));
        let service = HardwareService::with_gpu_backend(
            Box::new(FakeAuthorizer::new(AuthResult::Allow)),
            Box::new(auth.clone()),
            Box::new(backend.clone()),
            Some(EXPECTED_SENDER.into()),
        );
        let (_server, client) = connect_service(service).await.expect("P2P");
        let result = proxy(&client)
            .await
            .expect("proxy")
            .set_gpu_mode(1)
            .await
            .expect("reply");
        assert_eq!(result.requested_mode, 1);
        assert_eq!(
            result.returned_user_action,
            match action {
                SupergfxdUserAction::Logout => 0,
                SupergfxdUserAction::Reboot => 1,
                _ => 4,
            }
        );
        assert_eq!(result.outcome, expected);
        assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn backend_error_maps_to_failed_without_retry() {
    let auth = FakeAuthorizer::new(AuthResult::Allow);
    let backend = FakeGpuBackend::new(Err(ProviderError::Dbus("backend failure".into())));
    let err = handle_set_gpu_mode(&auth, Some(&backend), 1, EXPECTED_SENDER)
        .await
        .expect_err("backend error");
    assert!(matches!(err, zbus::fdo::Error::Failed(_)));
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn original_sender_reaches_authorizer() {
    let auth = FakeAuthorizer::new(AuthResult::Allow);
    let backend = FakeGpuBackend::new(Ok(observation(
        SupergfxdStagedState::Applied,
        SupergfxdUserAction::Nothing,
    )));
    let _result = handle_set_gpu_mode(&auth, Some(&backend), 1, EXPECTED_SENDER)
        .await
        .expect("allowed");
    assert_eq!(
        auth.sender.lock().unwrap().as_deref(),
        Some(EXPECTED_SENDER)
    );
}

#[tokio::test]
async fn p2p_test_is_bounded() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let auth = FakeAuthorizer::new(AuthResult::Allow);
        let backend = FakeGpuBackend::new(Ok(observation(
            SupergfxdStagedState::Pending,
            SupergfxdUserAction::Nothing,
        )));
        let service = HardwareService::with_gpu_backend(
            Box::new(FakeAuthorizer::new(AuthResult::Allow)),
            Box::new(auth),
            Box::new(backend),
            Some(EXPECTED_SENDER.into()),
        );
        let (_server, client) = connect_service(service).await.expect("P2P");
        proxy(&client)
            .await
            .expect("proxy")
            .set_gpu_mode(1)
            .await
            .expect("reply");
    })
    .await
    .expect("bounded P2P test");
}
