//! Build script: ensure `ui-chat/dist` exists for rust-embed (VL-UI-005).
//! 构建脚本：为 rust-embed 准备 `ui-chat/dist`（VL-UI-005）。

use std::fs;
use std::path::Path;

fn copy_recursive(src: &Path, dst: &Path) {
    if !dst.exists() {
        let _ = fs::create_dir_all(dst);
    }
    let Ok(entries) = fs::read_dir(src) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() {
            copy_recursive(&path, &target);
        } else if fs::copy(&path, &target).is_ok() {
            // copied
        }
    }
}

fn main() {
    let dist = Path::new("ui-chat/dist");
    let stub = Path::new("ui-chat/embed-stub");
    let index = dist.join("index.html");

    println!("cargo:rerun-if-changed=ui-chat/embed-stub");
    println!("cargo:rerun-if-changed=ui-chat/dist");

    if index.exists() {
        return;
    }

    if stub.join("index.html").exists() {
        copy_recursive(stub, dist);
        println!(
            "cargo:warning=ui-chat/dist missing; using embed-stub. Run npm run build in ui-chat/ for full UI."
        );
        return;
    }

    let _ = fs::create_dir_all(dist);
    let _ = fs::write(
        &index,
        "<!doctype html><title>VelaClaw Chat</title><p>Build ui-chat dist</p>",
    );
}
