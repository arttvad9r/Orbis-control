use orbis_providers::{NativeAsusEcoPreflightProvider, SystemNativeAsusEcoPreflightSource};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let provider =
        NativeAsusEcoPreflightProvider::new(SystemNativeAsusEcoPreflightSource::default());
    println!("snapshot={:#?}", provider.snapshot().await?);
    println!("readiness={:#?}", provider.readiness().await?);
    Ok(())
}
