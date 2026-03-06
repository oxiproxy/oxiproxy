use std::path::Path;

fn main() {
    // 确保 dist 目录存在，避免 rust-embed 在 CI 等未构建前端的环境中编译失败
    let dist_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../dist");
    if !dist_dir.exists() {
        std::fs::create_dir_all(&dist_dir).ok();
    }
}
