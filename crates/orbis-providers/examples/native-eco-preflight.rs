use orbis_providers::{
    NativeAsusEcoPreflightProvider, SystemNativeAsusEcoPreflightSource, plan_native_asus_eco,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let provider =
        NativeAsusEcoPreflightProvider::new(SystemNativeAsusEcoPreflightSource::default());
    let snapshot = provider.snapshot().await?;
    let assessment = plan_native_asus_eco(&snapshot);
    println!("snapshot={:#?}", snapshot);
    println!("plan={:#?}", assessment.plan);
    println!("hard_blockers={:#?}", assessment.hard_blockers);
    println!("release_required={:#?}", assessment.release_required);
    println!("informational={:#?}", assessment.informational);
    println!("unknown={:#?}", assessment.unknown);
    println!("readiness={:#?}", provider.readiness().await?);
    Ok(())
}
