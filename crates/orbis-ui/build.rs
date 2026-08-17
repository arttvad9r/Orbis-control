//! Компиляция Slint-интерфейса.
//!
//! `app-window.slint` импортирует компоненты относительно `ui/`, поэтому
//! include-path указывает на каталог `ui`.

use std::path::PathBuf;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let ui_dir = PathBuf::from(&manifest_dir).join("../../ui");
    let entry = ui_dir.join("app-window.slint");

    let entry_str = entry.to_string_lossy().to_string();

    slint_build::compile_with_config(
        &entry_str,
        slint_build::CompilerConfiguration::new().with_include_paths(vec![ui_dir]),
    )
    .expect("slint compile");
}
