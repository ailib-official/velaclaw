//! Build script: select embedded Web Chat assets folder (VL-UI-005).
//! 构建脚本：选择内嵌 Web Chat 资源目录（VL-UI-005）。
//!
//! Never writes into the source tree — required for `cargo publish` verification.

use std::path::Path;

fn main() {
    println!("cargo::rustc-check-cfg=cfg(chat_ui_dist)");

    let dist_index = Path::new("ui-chat/dist/index.html");
    let stub_index = Path::new("ui-chat/embed-stub/index.html");

    println!("cargo:rerun-if-changed=ui-chat/embed-stub");
    println!("cargo:rerun-if-changed=ui-chat/dist");

    if dist_index.exists() {
        println!("cargo:rustc-cfg=chat_ui_dist");
        return;
    }

    if !stub_index.exists() {
        panic!(
            "ui-chat/embed-stub/index.html missing; run `npm run build` in ui-chat/ or restore embed-stub"
        );
    }

    println!(
        "cargo:warning=ui-chat/dist missing; embedding ui-chat/embed-stub. Run npm run build in ui-chat/ for full UI."
    );
}
