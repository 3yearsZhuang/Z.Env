//! 有限的工具目录及接入账本；纯数据，不探测安装、不派发命令或推导执行权限。
use crate::domain::{
    node_tool_definition, PackageKind, Platform, ProviderId, ProviderPackage, ToolDefinition,
    ToolId,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EntryState {
    Integrated,
    NotIntegrated,
    Unsupported,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntrySupport {
    pub state: EntryState,
    pub reason: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Channel {
    pub provider_id: ProviderId,
    /// 登记的原生包身份，不表示包存在、安装器可用或目标具有写权限。
    pub packages: Vec<ProviderPackage>,
    pub inventory: EntrySupport,
    pub install_plan: EntrySupport,
    pub uninstall_plan: EntrySupport,
    pub binding_plan: EntrySupport,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogTool {
    pub definition: ToolDefinition,
    pub category: String,
    pub description: String,
    pub documentation_url: String,
    pub project_check: EntrySupport,
    pub channels: Vec<Channel>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCatalog {
    pub schema_version: u32,
    pub platform: Platform,
    pub coverage_note: String,
    pub tools: Vec<CatalogTool>,
}
fn support(state: EntryState, reason: &str) -> EntrySupport {
    EntrySupport {
        state,
        reason: reason.into(),
    }
}
fn pending() -> EntrySupport {
    support(
        EntryState::NotIntegrated,
        "统一来源和计划适配尚未接入；兼容入口不提供同等验证",
    )
}
fn node_support(platform: Platform, enabled: bool, reason: &str) -> EntrySupport {
    if platform != Platform::Macos {
        support(EntryState::Unsupported, "当前平台尚未接入统一 Node 流程")
    } else if enabled {
        support(EntryState::Integrated, reason)
    } else {
        support(EntryState::Unsupported, reason)
    }
}
fn node(platform: Platform) -> CatalogTool {
    let channels = [
        ProviderId::Mise,
        ProviderId::Brew,
        ProviderId::Nvm,
        ProviderId::Path,
    ]
    .into_iter()
    .map(|provider_id| {
        let (package_id, kind) = match provider_id {
            ProviderId::Brew => ("node", PackageKind::Formula),
            ProviderId::Path => ("node", PackageKind::Executable),
            _ => ("node", PackageKind::Runtime),
        };
        let managed = matches!(provider_id, ProviderId::Mise | ProviderId::Brew);
        Channel {
            provider_id,
            packages: if platform == Platform::Macos {
                vec![ProviderPackage {
                    provider_id,
                    package_id: package_id.into(),
                    kind,
                    platform,
                }]
            } else {
                vec![]
            },
            inventory: node_support(
                platform,
                true,
                if provider_id == ProviderId::Path {
                    "仅文件观测；未知 PATH 程序不自动执行"
                } else {
                    "读取来源目录与受限探针，保留健康及归属证据"
                },
            ),
            install_plan: node_support(
                platform,
                managed,
                if managed {
                    "已有计划入口；渠道可用性与版本承诺须在预览中重新核对"
                } else {
                    "此来源未提供安装计划"
                },
            ),
            uninstall_plan: node_support(
                platform,
                managed,
                if managed {
                    "已有计划入口；精确目标、归属及影响仍须逐项核对"
                } else {
                    "不从目录观测推导卸载权限"
                },
            ),
            binding_plan: node_support(
                platform,
                provider_id != ProviderId::Path,
                if provider_id != ProviderId::Path {
                    "已有绑定计划；健康实例、方式和作用域由计划服务核对"
                } else {
                    "未知 PATH 文件不提供绑定计划"
                },
            ),
        }
    })
    .collect();
    CatalogTool {
        definition: node_tool_definition(),
        category: "runtime".into(),
        description: "JavaScript 运行时".into(),
        documentation_url: "https://nodejs.org/en".into(),
        project_check: node_support(
            platform,
            true,
            "声明、候选、选择与受控上下文分别核对；用户终端和复杂继承仍可能未知",
        ),
        channels,
    }
}
fn planned(
    id: &str,
    name: &str,
    aliases: &[&str],
    executables: &[&str],
    category: &str,
    description: &str,
    url: &str,
) -> CatalogTool {
    CatalogTool {
        definition: ToolDefinition {
            tool_id: ToolId::from(id),
            display_name: name.into(),
            aliases: aliases.iter().map(|s| (*s).into()).collect(),
            executable_names: executables.iter().map(|s| (*s).into()).collect(),
        },
        category: category.into(),
        description: description.into(),
        documentation_url: url.into(),
        project_check: pending(),
        channels: [ProviderId::Mise, ProviderId::Brew, ProviderId::Path]
            .into_iter()
            .map(|provider_id| Channel {
                provider_id,
                packages: vec![],
                inventory: pending(),
                install_plan: pending(),
                uninstall_plan: pending(),
                binding_plan: pending(),
            })
            .collect(),
    }
}
fn python(platform: Platform) -> CatalogTool {
    let mut result = planned(
        "python",
        "Python",
        &[],
        &["python", "python3"],
        "runtime",
        "CPython 3 来源样板，项目虚拟环境单独管理",
        "https://www.python.org/",
    );
    result.definition = crate::domain::python_tool_definition();
    result.project_check = support(
        EntryState::NotIntegrated,
        "Python 声明、项目虚拟环境与实际解析尚未接入",
    );
    result.channels = crate::providers::python::PROVIDERS
        .into_iter()
        .map(|provider_id| Channel {
            provider_id,
            packages: if platform == Platform::Macos {
                vec![ProviderPackage {
                    provider_id,
                    package_id: if provider_id == ProviderId::Path {
                        "python3"
                    } else {
                        "python"
                    }
                    .into(),
                    kind: match provider_id {
                        ProviderId::Brew => PackageKind::Formula,
                        ProviderId::Path => PackageKind::Executable,
                        _ => PackageKind::Runtime,
                    },
                    platform,
                }]
            } else {
                vec![]
            },
            inventory: if platform == Platform::Macos {
                support(
                    EntryState::Integrated,
                    if provider_id == ProviderId::Path {
                        "仅 Python 文件观测；未知 PATH 程序不执行"
                    } else {
                        "只读 CPython 3 来源目录和隔离探针，不加载 site 或项目配置"
                    },
                )
            } else {
                support(EntryState::Unsupported, "当前平台尚未接入统一 Python 来源")
            },
            install_plan: if platform == Platform::Macos && provider_id != ProviderId::Path {
                support(
                    EntryState::Integrated,
                    match provider_id {
                        ProviderId::Mise => {
                            "原生 core:python 精确预编译安装；执行前按计划重验来源与副作用"
                        }
                        ProviderId::Uv => {
                            "uv 托管 CPython 精确版本，--no-bin 与受控安装目录；执行前重验能力、别名与产物"
                        }
                        _ => "已有安装计划预览；渠道执行与副作用验证未完成，安装执行未开放",
                    },
                )
            } else {
                support(
                    EntryState::Unsupported,
                    "此来源或平台尚未提供 Python 安装计划",
                )
            },
            uninstall_plan: if platform == Platform::Macos
                && matches!(provider_id, ProviderId::Mise | ProviderId::Uv)
            {
                support(
                    EntryState::Integrated,
                    if provider_id == ProviderId::Mise {
                        "原生管理器记录、收据与精确实例一致后才允许卸载；不删除外部链接"
                    } else {
                        "uv 精确实例、真实目录与探测结果一致后才允许卸载；0 退出不当作删除证明"
                    },
                )
            } else {
                support(
                    EntryState::NotIntegrated,
                    "此来源 Python 卸载归属和影响门禁尚未接入",
                )
            },
            binding_plan: support(
                EntryState::NotIntegrated,
                "Python 绑定、项目虚拟环境与作用域验证尚未接入",
            ),
        })
        .collect();
    result
}
pub fn catalog(platform: Platform) -> ToolCatalog {
    ToolCatalog { schema_version:1,platform,coverage_note:"首批 7 项工具定义；目录收录不表示已安装、渠道可用或拥有执行权限。其他工具仍可在兼容入口查看。".into(),tools:vec![
        node(platform),
        python(platform),
        planned("java","Java",&[],&["java","javac"],"runtime","JVM 与 Java 开发工具","https://dev.java/"),
        planned("go","Go",&["golang"],&["go"],"runtime","Go 工具链","https://go.dev/"),
        planned("rust","Rust",&[],&["rustc"],"runtime","Rust 编译器工具链；Cargo 保留独立工具身份","https://rust-lang.org/"),
        planned("git","Git",&[],&["git"],"cli","版本控制命令行工具","https://git-scm.com/"),
        planned("uv","uv",&[],&["uv"],"cli","Python 项目与包管理工具","https://docs.astral.sh/uv/"),
    ] }
}
pub fn current_catalog() -> ToolCatalog {
    catalog(if cfg!(target_os = "macos") {
        Platform::Macos
    } else if cfg!(windows) {
        Platform::Windows
    } else {
        Platform::Linux
    })
}
/// 只使用显式别名，不根据可执行文件名、子串或包名猜测身份。
pub fn tool<'a>(catalog: &'a ToolCatalog, raw: &str) -> Option<&'a CatalogTool> {
    let key = raw.trim().to_ascii_lowercase();
    catalog.tools.iter().find(|t| {
        t.definition.tool_id.0.eq_ignore_ascii_case(&key)
            || t.definition
                .aliases
                .iter()
                .any(|a| a.eq_ignore_ascii_case(&key))
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn definitions_keep_runtime_package_and_executable_identities_distinct() {
        let c = catalog(Platform::Macos);
        assert_eq!(tool(&c, " Node.js ").unwrap().definition.tool_id.0, "node");
        assert_eq!(tool(&c, "golang").unwrap().definition.tool_id.0, "go");
        for raw in ["cargo", "rustup", "python3", "node@22", "pypy", "some-node"] {
            assert!(tool(&c, raw).is_none(), "{raw}");
        }
        let mut identities = std::collections::BTreeMap::new();
        for t in &c.tools {
            for id in std::iter::once(&t.definition.tool_id.0).chain(&t.definition.aliases) {
                let old = identities.insert(id.to_lowercase(), t.definition.tool_id.0.clone());
                assert!(old.is_none_or(|v| v == t.definition.tool_id.0));
            }
        }
    }
    #[test]
    fn platform_and_provider_entries_do_not_grant_installation_or_execution_permissions() {
        for platform in [Platform::Macos, Platform::Windows, Platform::Linux] {
            let c = catalog(platform);
            for t in &c.tools {
                for channel in &t.channels {
                    if !matches!(t.definition.tool_id.0.as_str(), "node" | "python") {
                        assert!(channel.packages.is_empty());
                        assert_eq!(channel.inventory.state, EntryState::NotIntegrated);
                        assert_eq!(channel.install_plan.state, EntryState::NotIntegrated);
                    } else if platform != Platform::Macos {
                        assert!(channel.packages.is_empty());
                        assert_eq!(channel.inventory.state, EntryState::Unsupported);
                        assert_ne!(channel.install_plan.state, EntryState::Integrated);
                    }
                    if matches!(channel.provider_id, ProviderId::Nvm | ProviderId::Path) {
                        assert_ne!(channel.install_plan.state, EntryState::Integrated);
                        assert_ne!(channel.uninstall_plan.state, EntryState::Integrated);
                    }
                    assert!(!channel.inventory.reason.is_empty());
                    assert!(!channel.install_plan.reason.is_empty());
                }
            }
        }
    }
    #[test]
    fn contract_covers_all_platforms_and_roundtrips_without_machine_state() {
        let fixture: Vec<ToolCatalog> = serde_json::from_str(include_str!(
            "../../../fixtures/tool-catalog/contract-v1.json"
        ))
        .unwrap();
        assert_eq!(fixture.len(), 3);
        for c in fixture {
            let value = serde_json::to_value(&c).unwrap();
            assert_eq!(value, serde_json::to_value(catalog(c.platform)).unwrap());
            assert!(value.get("installations").is_none());
            assert!(value.get("canExecute").is_none());
        }
    }
}
