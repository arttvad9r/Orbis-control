//! Entry point orbis-sessiond: запускает discovered read-only UPower-backed
//! session D-Bus service и ждёт SIGINT/SIGTERM (semantics — в `runtime`).

use orbis_sessiond::runtime::{RuntimeError, run_discovered_sessiond};

#[tokio::main]
async fn main() -> Result<(), RuntimeError> {
    run_discovered_sessiond().await
}
