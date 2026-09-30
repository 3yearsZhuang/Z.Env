// 开发缓存治理：展示常见开发缓存目录体积，清理只走各工具的官方命令。
// 体积统计带条目预算防呆，超预算返回下限值并标记近似；无官方清理命令的项（cargo
// registry）只展示不动手。
use crate::error::AppError;
use serde::Serialize;

/// 体积统计的目录条目预算（达到预算即截断，结果为下限）。
const SIZE_BUDGET: usize = 200_000;

#[cfg(windows)]
const PIP: &str = "python";
#[cfg(not(windows))]
const PIP: &str = "python3";

/// 单个缓存项。exists=false 表示工具未装或缓存尚未生成。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheInfo {
    pub id: String,
    pub name: String,
    pub path: String,
    pub exists: bool,
    pub size_bytes: u64,
    /// 体积因预算提前截断，size_bytes 为下限。
    pub approx: bool,
    /// 是否有官方清理命令。
    pub cleanable: bool,
}

/// 列出本机常见开发缓存：探测命令成功的才列出（没装的工具不占位）。
pub fn list() -> Vec<CacheInfo> {
    let mut out = Vec::new();
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from);

    if let Some(p) = probe_path("brew", &["--cache"]) {
        add(&mut out, "brew", "Homebrew", Some(p), true);
    }
    if let Some(h) = &home {
        let xdg = std::env::var("XDG_CACHE_HOME")
            .ok()
            .filter(|s| !s.is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| h.join(".cache"));
        add(&mut out, "mise", "mise", Some(xdg.join("mise")), true);
    }
    if let Some(p) = probe_path("npm", &["config", "get", "cache"]) {
        add(&mut out, "npm", "npm", Some(p), true);
    }
    // yarn v1 的缓存目录查询；v2+ 无此命令则自动跳过
    if let Some(p) = probe_path("yarn", &["cache", "dir"]) {
        add(&mut out, "yarn", "Yarn", Some(p), true);
    }
    if let Some(p) = probe_path(PIP, &["-m", "pip", "cache", "dir"]) {
        add(&mut out, "pip", "pip", Some(p), true);
    }
    if let Some(p) = probe_path("uv", &["cache", "dir"]) {
        add(&mut out, "uv", "uv", Some(p), true);
    }
    if let Some(p) = probe_path("go", &["env", "GOCACHE"]) {
        add(&mut out, "gobuild", "Go 构建缓存", Some(p), true);
    }
    if let Some(h) = &home {
        add(
            &mut out,
            "cargo",
            "Cargo registry",
            Some(h.join(".cargo/registry")),
            false,
        );
    }
    out
}

/// 用官方命令清理指定缓存（长操作，10 分钟超时）。
pub fn clean(id: &str) -> Result<String, AppError> {
    let (prog, args): (&str, Vec<&str>) = match id {
        "brew" => ("brew", vec!["cleanup", "-s"]),
        "mise" => ("mise", vec!["cache", "clear"]),
        "npm" => ("npm", vec!["cache", "clean", "--force"]),
        "yarn" => ("yarn", vec!["cache", "clean"]),
        "pip" => (PIP, vec!["-m", "pip", "cache", "purge"]),
        "uv" => ("uv", vec!["cache", "clean"]),
        "gobuild" => ("go", vec!["clean", "-cache"]),
        "cargo" => return Err(AppError::Unsupported(
            "cargo registry 无官方一键清理，可按需删除 ~/.cargo/registry/cache 后由 cargo 自动重建"
                .into(),
        )),
        other => return Err(AppError::Unsupported(format!("未知缓存项: {other}"))),
    };
    let out = run_long(prog, &args)?;
    if !out.status.success() {
        return Err(AppError::Other(format!(
            "清理失败：{}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(format!("已清理 {id} 缓存"))
}

/// 追加一个缓存项并统计体积（路径缺失也占位，前端展示"未生成"）。
fn add(
    out: &mut Vec<CacheInfo>,
    id: &str,
    name: &str,
    path: Option<std::path::PathBuf>,
    cleanable: bool,
) {
    let Some(path) = path else { return };
    let exists = path.exists();
    let (size_bytes, approx) = if exists {
        let mut budget = SIZE_BUDGET;
        dir_size(&path, &mut budget)
    } else {
        (0, false)
    };
    out.push(CacheInfo {
        id: id.to_string(),
        name: name.to_string(),
        path: path.display().to_string(),
        exists,
        size_bytes,
        approx,
        cleanable,
    });
}

/// 递归统计目录字节大小；符号链接不跟随；预算耗尽时返回近似标记（值为下限）。
fn dir_size(dir: &std::path::Path, budget: &mut usize) -> (u64, bool) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (0, false);
    };
    let mut total = 0u64;
    let mut approx = false;
    for e in entries.flatten() {
        if *budget == 0 {
            return (total, true);
        }
        *budget -= 1;
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            let (s, a) = dir_size(&e.path(), budget);
            total += s;
            approx = approx || a;
        } else if let Ok(m) = e.metadata() {
            total += m.len();
        }
    }
    (total, approx)
}

/// 运行命令取完整输出（10 分钟超时，供清理等长操作）。
fn run_long(prog: &str, args: &[&str]) -> Result<std::process::Output, AppError> {
    let (prog2, args2) = (
        prog.to_string(),
        args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
    );
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(std::process::Command::new(&prog2).args(&args2).output());
    });
    rx.recv_timeout(std::time::Duration::from_secs(600))
        .map_err(|_| AppError::Other("清理命令执行超时（10 分钟）".into()))?
        .map_err(|e| AppError::Io(format!("无法执行 {prog}: {e}")))
}

/// 运行探测命令取首行作为路径（命令缺失/失败/空输出返回 None；10 秒超时）。
fn probe_path(prog: &str, args: &[&str]) -> Option<std::path::PathBuf> {
    let (prog2, args2) = (
        prog.to_string(),
        args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
    );
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(std::process::Command::new(&prog2).args(&args2).output());
    });
    let out = rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .ok()?
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()?
        .trim()
        .to_string();
    if line.is_empty() {
        None
    } else {
        Some(std::path::PathBuf::from(line))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dir_size_sums_files_and_skips_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("a.bin"), vec![0u8; 100]).unwrap();
        std::fs::create_dir(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/b.bin"), vec![0u8; 50]).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("a.bin"), root.join("link.bin")).unwrap();
        let mut budget = SIZE_BUDGET;
        let (size, approx) = dir_size(root, &mut budget);
        assert_eq!(size, 150);
        assert!(!approx);
    }

    #[test]
    fn dir_size_reports_approx_when_budget_exhausted() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        for i in 0..5 {
            std::fs::write(root.join(format!("f{i}.bin")), vec![0u8; 10]).unwrap();
        }
        let mut budget = 2;
        let (_size, approx) = dir_size(root, &mut budget);
        assert!(approx);
    }

    #[test]
    fn clean_rejects_unknown_and_non_cleanable() {
        assert!(matches!(
            clean("browser-history"),
            Err(AppError::Unsupported(_))
        ));
        assert!(matches!(clean("cargo"), Err(AppError::Unsupported(_))));
    }
}
