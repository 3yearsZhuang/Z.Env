//! Python 只读查询；--fixture 生成跨端契约，否则输出真实本机观测（仅保存本地）。
use mise_gui_lib::{
    domain::*,
    inventory::{
        merge_inventory,
        query::{InventoryQueryService, InventoryTool},
    },
    providers::python::{PythonScanOptions, PROVIDERS},
};
fn fixture() -> InventorySnapshot {
    let scans = PROVIDERS
        .into_iter()
        .map(|provider_id| {
            let observations = if matches!(
                provider_id,
                ProviderId::Mise | ProviderId::Pyenv | ProviderId::Path
            ) {
                let path = if provider_id == ProviderId::Path {
                    "/fixture/bin/python3"
                } else if provider_id == ProviderId::Mise {
                    "/fixture/mise/3.13.8/bin/python3"
                } else {
                    "/fixture/pyenv/3.12.9/bin/python3"
                };
                vec![InstallationObservation {
                    provider_package: ProviderPackage {
                        provider_id,
                        package_id: if provider_id == ProviderId::Path {
                            "python3"
                        } else {
                            "python"
                        }
                        .into(),
                        kind: if provider_id == ProviderId::Path {
                            PackageKind::Executable
                        } else {
                            PackageKind::Runtime
                        },
                        platform: Platform::Macos,
                    },
                    tool_id: ToolId::from("python"),
                    locator: path.into(),
                    executable_path: Some(path.into()),
                    canonical_executable_path: Some(
                        if provider_id == ProviderId::Path {
                            "/fixture/mise/3.13.8/bin/python3"
                        } else {
                            path
                        }
                        .into(),
                    ),
                    version: if provider_id == ProviderId::Path {
                        None
                    } else {
                        Some(
                            if provider_id == ProviderId::Mise {
                                "3.13.8"
                            } else {
                                "3.12.9"
                            }
                            .into(),
                        )
                    },
                    architecture: if provider_id == ProviderId::Path {
                        None
                    } else {
                        Some("arm64".into())
                    },
                    health: if provider_id == ProviderId::Path {
                        Health::Unverified
                    } else {
                        Health::Healthy
                    },
                    ownership: Ownership::Observed,
                    evidence: vec![ObservationEvidence {
                        kind: if provider_id == ProviderId::Path {
                            "pythonPathFile"
                        } else {
                            "pythonIsolatedProbe"
                        }
                        .into(),
                        path: Some(path.into()),
                        detail: Some("合成契约夹具，不代表本机实测".into()),
                    }],
                }]
            } else {
                vec![]
            };
            ProviderScan {
                report: ProviderReport {
                    provider_id,
                    status: ProviderStatus::Ok,
                    observed_at: 100,
                    stale: false,
                    coverage: ScanCoverage {
                        tool_ids: vec![ToolId::from("python")],
                        complete: true,
                        truncated: false,
                    },
                    error: None,
                },
                observations,
            }
        })
        .collect();
    merge_inventory("python-fixture".into(), 100, scans).unwrap()
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let snapshot = if std::env::args().nth(1).as_deref() == Some("--fixture") {
        fixture()
    } else {
        InventoryQueryService::default()
            .begin_tool("python-native".into(), None, InventoryTool::Python)?
            .execute_python({
                let mut options = PythonScanOptions::from_environment()?;
                if let Some(root) =
                    std::env::var_os("ZENV_ACCEPTANCE_ROOT").map(std::path::PathBuf::from)
                {
                    if !root.is_absolute()
                        || !options.common.home.starts_with(&root)
                        || !options.common.mise_data_dir.starts_with(&root)
                        || !options.pyenv_root.starts_with(&root)
                        || !options.uv_python_dir.starts_with(&root)
                        || !root.join("ALLOW_DISPOSABLE_OPERATIONS").is_file()
                    {
                        return Err("隔离验收根目录无效".into());
                    }
                    options.common.brew_cellars = vec![root.join("Cellar")];
                }
                options
            })?
    };
    println!("{}", serde_json::to_string_pretty(&snapshot)?);
    Ok(())
}
