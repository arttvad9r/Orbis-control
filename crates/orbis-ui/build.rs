//! Compile the audited Orbis Control Slint interface.
//!
//! The entrypoint re-exports the stable public types expected by Rust while
//! selecting the screenshot-audited window implementations under `ui/audited`.

use std::path::PathBuf;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let ui_dir = PathBuf::from(&manifest_dir).join("../../ui");
    let entry = ui_dir.join("app-entry.slint");

    let entry_str = entry.to_string_lossy().to_string();

    // Use one cross-platform widget style instead of Linux `native`, which may
    // select Qt and ignore runtime Palette.color-scheme overrides. Fluent keeps
    // ComboBox/SpinBox/ScrollView consistent with Orbis dark/light theme logic.
    let config = slint_build::CompilerConfiguration::new()
        .with_include_paths(vec![ui_dir])
        .with_style("fluent".into());

    slint_build::compile_with_config(&entry_str, config).expect("slint compile");
}
