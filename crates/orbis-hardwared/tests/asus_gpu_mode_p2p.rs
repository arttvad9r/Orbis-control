use std::sync::{Arc, Mutex};

use orbis_hardwared::asus_gpu_mode::{
    ASUSD_DGPU_DISABLE_PATH, ASUSD_GPU_MUX_MODE_PATH, AsusGpuMutationOperation,
    AsusdGpuMutationClient,
};
use orbis_providers::ProviderError;
use orbis_providers::asus_gpu_mode::{AsusGpuMode, ProductGpuOutcome};
use zbus::connection::Builder;

#[derive(Clone)]
struct FakeAttribute {
    name: &'static str,
    current: Arc<Mutex<i32>>,
    queued: Arc<Mutex<Option<i32>>>,
    setters: Arc<Mutex<Vec<i32>>>,
    setter_order: Arc<Mutex<Vec<&'static str>>>,
    failure: Arc<Mutex<Option<String>>>,
    retain_queued_on_set: Arc<Mutex<bool>>,
}

impl FakeAttribute {
    fn new(name: &'static str, current: i32, queued: Option<i32>) -> Self {
        Self {
            name,
            current: Arc::new(Mutex::new(current)),
            queued: Arc::new(Mutex::new(queued)),
            setters: Arc::new(Mutex::new(Vec::new())),
            setter_order: Arc::new(Mutex::new(Vec::new())),
            failure: Arc::new(Mutex::new(None)),
            retain_queued_on_set: Arc::new(Mutex::new(false)),
        }
    }

    fn set_failure(&self, message: &str) {
        *self.failure.lock().unwrap() = Some(message.to_owned());
    }

    fn setters(&self) -> Vec<i32> {
        self.setters.lock().unwrap().clone()
    }

    fn share_setter_order_with(&mut self, other: &Self) {
        self.setter_order = other.setter_order.clone();
    }

    fn retain_queued_on_set(&self) {
        *self.retain_queued_on_set.lock().unwrap() = true;
    }

    fn setter_order(&self) -> Vec<&'static str> {
        self.setter_order.lock().unwrap().clone()
    }
}

#[zbus::interface(name = "xyz.ljones.AsusArmoury")]
impl FakeAttribute {
    #[zbus(property)]
    async fn current_value(&self) -> zbus::fdo::Result<i32> {
        Ok(*self.current.lock().unwrap())
    }

    #[zbus(property)]
    async fn queued_gpu_value(&self) -> zbus::fdo::Result<i32> {
        Ok(self.queued.lock().unwrap().unwrap_or(-1))
    }

    #[zbus(property)]
    async fn set_current_value(&mut self, value: i32) -> zbus::fdo::Result<()> {
        self.setters.lock().unwrap().push(value);
        self.setter_order.lock().unwrap().push(self.name);
        if let Some(message) = self.failure.lock().unwrap().clone() {
            return Err(zbus::fdo::Error::Failed(message));
        }
        if !*self.retain_queued_on_set.lock().unwrap() {
            *self.queued.lock().unwrap() = Some(value);
        }
        Ok(())
    }
}

async fn client(
    dgpu: FakeAttribute,
    mux: FakeAttribute,
) -> (zbus::Connection, AsusdGpuMutationClient) {
    let (server_stream, client_stream) = std::os::unix::net::UnixStream::pair().unwrap();
    let server = Builder::unix_stream(server_stream)
        .server(zbus::Guid::generate())
        .expect("server builder")
        .p2p()
        .serve_at(ASUSD_DGPU_DISABLE_PATH, dgpu)
        .expect("dgpu object")
        .serve_at(ASUSD_GPU_MUX_MODE_PATH, mux)
        .expect("mux object")
        .build();
    let (server, connection) =
        tokio::try_join!(server, Builder::unix_stream(client_stream).p2p().build()).unwrap();
    let typed = AsusdGpuMutationClient::new(connection.clone());
    std::mem::forget(server);
    (connection, typed)
}

fn operation(client: AsusdGpuMutationClient) -> AsusGpuMutationOperation<AsusdGpuMutationClient> {
    AsusGpuMutationOperation::new(client)
}

#[tokio::test]
async fn queues_integrated_in_deterministic_order_and_confirms_pair() {
    let dgpu = FakeAttribute::new("dgpu_disable", 0, None);
    let mut mux = FakeAttribute::new("gpu_mux_mode", 1, None);
    mux.share_setter_order_with(&dgpu);
    let dgpu_view = dgpu.clone();
    let mux_view = mux.clone();
    let (_connection, client) = client(dgpu, mux).await;

    let result = operation(client)
        .set_mode(AsusGpuMode::Integrated)
        .await
        .unwrap();

    assert_eq!(result.outcome, ProductGpuOutcome::RebootRequired);
    assert_eq!(dgpu_view.setters(), vec![1]);
    assert_eq!(mux_view.setters(), vec![1]);
    assert_eq!(
        dgpu_view.setter_order(),
        vec!["dgpu_disable", "gpu_mux_mode"]
    );
}

#[tokio::test]
async fn already_active_does_not_set_attributes() {
    let dgpu = FakeAttribute::new("dgpu_disable", 1, None);
    let mux = FakeAttribute::new("gpu_mux_mode", 1, None);
    let dgpu_view = dgpu.clone();
    let mux_view = mux.clone();
    let (_connection, client) = client(dgpu, mux).await;

    let result = operation(client)
        .set_mode(AsusGpuMode::Integrated)
        .await
        .unwrap();

    assert_eq!(result.outcome, ProductGpuOutcome::AlreadyActive);
    assert!(dgpu_view.setters().is_empty());
    assert!(mux_view.setters().is_empty());
}

#[tokio::test]
async fn first_setter_failure_preserves_error_and_skips_second() {
    let dgpu = FakeAttribute::new("dgpu_disable", 0, None);
    dgpu.set_failure("first setter failed");
    let mux = FakeAttribute::new("gpu_mux_mode", 1, None);
    let mux_view = mux.clone();
    let (_connection, client) = client(dgpu, mux).await;

    let error = operation(client)
        .set_mode(AsusGpuMode::Integrated)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("first setter failed"));
    assert!(mux_view.setters().is_empty());
}

#[tokio::test]
async fn second_setter_failure_is_unknown_and_is_not_retried() {
    let dgpu = FakeAttribute::new("dgpu_disable", 0, None);
    let mux = FakeAttribute::new("gpu_mux_mode", 1, None);
    mux.set_failure("second setter failed");
    let dgpu_view = dgpu.clone();
    let mux_view = mux.clone();
    let (_connection, client) = client(dgpu, mux).await;

    let error = operation(client)
        .set_mode(AsusGpuMode::Integrated)
        .await
        .unwrap_err();
    assert!(matches!(error, ProviderError::Conflict(_)));
    assert_eq!(dgpu_view.setters(), vec![1]);
    assert_eq!(mux_view.setters(), vec![1]);
}

#[tokio::test]
async fn partial_queued_readback_is_unknown() {
    let dgpu = FakeAttribute::new("dgpu_disable", 0, None);
    let mux = FakeAttribute::new("gpu_mux_mode", 1, None);
    mux.retain_queued_on_set();
    let (_connection, client) = client(dgpu, mux).await;

    let result = operation(client)
        .set_mode(AsusGpuMode::Integrated)
        .await
        .unwrap();
    assert_eq!(result.outcome, ProductGpuOutcome::Unknown);
}

#[tokio::test]
async fn stale_contradictory_queue_is_replaced_by_complete_target() {
    let dgpu = FakeAttribute::new("dgpu_disable", 0, Some(0));
    let mux = FakeAttribute::new("gpu_mux_mode", 1, Some(0));
    let (_connection, client) = client(dgpu, mux).await;

    let result = operation(client)
        .set_mode(AsusGpuMode::Integrated)
        .await
        .unwrap();
    assert_eq!(result.outcome, ProductGpuOutcome::RebootRequired);
}

#[tokio::test]
async fn unsupported_target_is_rejected_before_setters() {
    let dgpu = FakeAttribute::new("dgpu_disable", 0, None);
    let mux = FakeAttribute::new("gpu_mux_mode", 1, None);
    let dgpu_view = dgpu.clone();
    let mux_view = mux.clone();
    let (_connection, client) = client(dgpu, mux).await;

    let error = operation(client)
        .set_mode(AsusGpuMode::Incomplete)
        .await
        .unwrap_err();
    assert!(matches!(error, ProviderError::InvalidRequest(_)));
    assert!(dgpu_view.setters().is_empty());
    assert!(mux_view.setters().is_empty());
}
