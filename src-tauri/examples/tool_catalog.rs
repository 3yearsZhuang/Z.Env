//! 纯目录契约导出；不读取机器安装、环境变量或配置。
use mise_gui_lib::{domain::Platform, tool_catalog::catalog};
fn main() {
    println!(
        "{}",
        serde_json::to_string_pretty(&[
            catalog(Platform::Macos),
            catalog(Platform::Windows),
            catalog(Platform::Linux)
        ])
        .unwrap()
    );
}
