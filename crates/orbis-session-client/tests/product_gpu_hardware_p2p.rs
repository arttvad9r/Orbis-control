//! Private P2P coverage for the typed Hardware1 product-GPU source.
//!
//! Uses only a local Unix-stream pair; no system/session bus or real hardware.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use orbis_hardwared::{DBUS_OBJECT_PATH, ProductGpuMutationResult};
use orbis_providers::ProviderError;
use orbis_session_client::{HardwareProductGpuSource, ZbusHardwareProductGpuSource};
use zbus::connection::Builder;

#[derive(Clone)]
struct HardwareObject {
    requests: Arc<Mutex<Vec<u32>>>,
    result: zbus::fdo::Result<ProductGpuMutationResult>,
}

#[zbus::interface(name = "io.github.orbiscontrol.Hardware1")]
impl HardwareObject {
    async fn set_product_gpu_mode(
        &self,
        requested_mode: u32,
    ) -> zbus::fdo::Result<ProductGpuMutationResult> {
        self.requests.lock().unwrap().push(requested_mode);
        self.result.clone()
    }
}

async fn connect_hardware(
    object: HardwareObject,
) -> Result<(zbus::Connection, zbus::Connection), Box<dyn std::error::Error + Send + Sync>> {
    let (server_stream, client_stream) = std::os::unix::net::UnixStream::pair()?;
    let server = Builder::unix_stream(server_stream)
        .server(zbus::Guid::generate())?
        .p2p()
        .serve_at(DBUS_OBJECT_PATH, object)?
        .build();
    let client = Builder::unix_stream(client_stream).p2p().build();
    Ok(tokio::try_join!(server, client)?)
}

#[tokio::test]
async fn product_gpu_source_sends_exact_target_and_preserves_result_fields() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let expected = ProductGpuMutationResult {
            requested_mode: u32::MAX,
            current_mode: 0,
            queued_mode: u32::MAX,
            outcome: 3,
            reboot_required: true,
        };
        let (_server, client) = connect_hardware(HardwareObject {
            requests: requests.clone(),
            result: Ok(expected),
        })
        .await
        .expect("p2p");
        let source = ZbusHardwareProductGpuSource::new(client);

        let result = source.set_product_gpu_mode(2).await.expect("reply");

        assert_eq!(result, expected);
        assert_eq!(*requests.lock().unwrap(), vec![2]);
    })
    .await
    .expect("p2p test timeout");
}

#[tokio::test]
async fn product_gpu_source_maps_remote_failure_without_retry() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let (_server, client) = connect_hardware(HardwareObject {
            requests: requests.clone(),
            result: Err(zbus::fdo::Error::Failed("scripted failure".into())),
        })
        .await
        .expect("p2p");
        let source = ZbusHardwareProductGpuSource::new(client);

        assert!(matches!(
            source.set_product_gpu_mode(1).await,
            Err(ProviderError::Dbus(_))
        ));
        assert_eq!(*requests.lock().unwrap(), vec![1]);
    })
    .await
    .expect("p2p test timeout");
}
