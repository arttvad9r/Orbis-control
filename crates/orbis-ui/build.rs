//! Компиляция Slint-интерфейса.
//!
//! `app-window.slint` импортирует компоненты относительно `ui/`, поэтому
//! include-path указывает на каталог `ui`.

use std::path::PathBuf;

fn main() {
    let ui_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../ui");
    let entry = ui_dir.join("app-window.slint");
    slint_build::compile_with_config(
        entry.to_str().expect("path"),
        slint_build::CompilerConfiguration::new().with_include_paths(vec![ui_dir]),
    )
    .expect("slint compile");
}
