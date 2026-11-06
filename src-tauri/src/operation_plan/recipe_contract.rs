//! 严格的 Recipe v2 传输模型；当前请求仅校验后返回 Disabled，不签发执行批准。
use super::{
    recipe_record::{
        validate_recipe_steps, RecipeStepIdentityV2, RecipeSubjectV2, MAX_RECIPE_ITEMS,
    },
    BindingScope, BindingStrategy, InventoryQueryService, OperationPlanService, OperationPlanV2,
    PlanError, PlanRequestV2, StepGraphEntryV2,
};
use crate::{
    domain::{PackageKind, Platform, ProviderId},
    inventory::query::InventoryTool,
};
use serde::{
    de::{MapAccess, SeqAccess, Visitor},
    Deserialize, Deserializer, Serialize, Serializer,
};
use std::collections::BTreeSet;

const MAX_PATH_BYTES: usize = 4096;
const MAX_SAFE_TIMESTAMP: u64 = 9_007_199_254_740_991;
const MAX_DECLARATION_FILES: usize = 8;
const MAX_REASON_BYTES: usize = 512;
const MAX_DIFF_VALUE_BYTES: usize = 128;

/// 保留旧 Tool struct API；解码明确按 subject.kind 分派，非法 Recipe 不降级为 Tool。
#[derive(Clone, Debug)]
pub enum AnyPlanRequestV2 {
    Tool(PlanRequestV2),
    Recipe(RecipePlanRequestV2),
}

impl Serialize for AnyPlanRequestV2 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Tool(request) => request.serialize(serializer),
            Self::Recipe(request) => request.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for AnyPlanRequestV2 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = UniqueJsonValue::deserialize(deserializer)?.0;
        let kind = value
            .get("subject")
            .and_then(|subject| subject.get("kind"))
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| serde::de::Error::custom("v2 request requires subject.kind"))?;
        let request = match kind {
            "tool" => serde_json::from_value::<PlanRequestV2>(value)
                .map(Self::Tool)
                .map_err(serde::de::Error::custom)?,
            "recipe" => serde_json::from_value::<RecipePlanRequestV2>(value)
                .map(Self::Recipe)
                .map_err(serde::de::Error::custom)?,
            _ => return Err(serde::de::Error::custom("unsupported v2 subject.kind")),
        };
        request.validate().map_err(serde::de::Error::custom)?;
        Ok(request)
    }
}

/// 在转为 Value 前递归拒绝重复键，保留旧 Tool 直接 serde 解码的严格拒绝行为。
/// 已经被传输层折叠的 Value 无法恢复原始键；这里保证原始 JSON 合同的解码边界。
struct UniqueJsonValue(serde_json::Value);

impl<'de> Deserialize<'de> for UniqueJsonValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct UniqueValueVisitor;

        impl<'de> Visitor<'de> for UniqueValueVisitor {
            type Value = UniqueJsonValue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a JSON value with unique object keys")
            }

            fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
                Ok(UniqueJsonValue(serde_json::Value::Bool(value)))
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
                Ok(UniqueJsonValue(serde_json::Value::Number(value.into())))
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
                Ok(UniqueJsonValue(serde_json::Value::Number(value.into())))
            }

            fn visit_i128<E: serde::de::Error>(self, value: i128) -> Result<Self::Value, E> {
                let deserializer = serde::de::value::I128Deserializer::new(value);
                serde_json::Number::deserialize(deserializer)
                    .map(serde_json::Value::Number)
                    .map(UniqueJsonValue)
            }

            fn visit_u128<E: serde::de::Error>(self, value: u128) -> Result<Self::Value, E> {
                let deserializer = serde::de::value::U128Deserializer::new(value);
                serde_json::Number::deserialize(deserializer)
                    .map(serde_json::Value::Number)
                    .map(UniqueJsonValue)
            }

            fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> {
                Ok(UniqueJsonValue(serde_json::Number::from_f64(value).map_or(
                    serde_json::Value::Null,
                    serde_json::Value::Number,
                )))
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                self.visit_string(value.to_owned())
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
                Ok(UniqueJsonValue(serde_json::Value::String(value)))
            }

            fn visit_none<E>(self) -> Result<Self::Value, E> {
                Ok(UniqueJsonValue(serde_json::Value::Null))
            }

            fn visit_some<D: Deserializer<'de>>(
                self,
                deserializer: D,
            ) -> Result<Self::Value, D::Error> {
                UniqueJsonValue::deserialize(deserializer)
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(UniqueJsonValue(serde_json::Value::Null))
            }

            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = sequence.next_element::<UniqueJsonValue>()? {
                    values.push(value.0);
                }
                Ok(UniqueJsonValue(serde_json::Value::Array(values)))
            }

            fn visit_map<A: MapAccess<'de>>(self, mut object: A) -> Result<Self::Value, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some(key) = object.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(serde::de::Error::custom(format!(
                            "duplicate JSON object key: {key}"
                        )));
                    }
                    let value = object.next_value::<UniqueJsonValue>()?;
                    values.insert(key, value.0);
                }
                Ok(UniqueJsonValue(serde_json::Value::Object(values)))
            }
        }

        deserializer.deserialize_any(UniqueValueVisitor)
    }
}

impl AnyPlanRequestV2 {
    pub fn validate(&self) -> Result<(), PlanError> {
        match self {
            Self::Tool(request) => request.validate().map(|_| ()),
            Self::Recipe(request) => request.validate(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecipeBindingOptionsV2 {
    pub scope: BindingScope,
    pub strategy: BindingStrategy,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RecipeSelectionV2 {
    Install {},
    Reuse { installation_id: String },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecipeRequestItemV2 {
    pub tool_id: InventoryTool,
    pub version: String,
    pub snapshot_request_id: String,
    pub selection: RecipeSelectionV2,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecipeRequestV2 {
    pub project_path: String,
    pub binding: RecipeBindingOptionsV2,
    pub items: Vec<RecipeRequestItemV2>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", try_from = "RecipePlanRequestWire")]
pub struct RecipePlanRequestV2 {
    pub schema_version: u32,
    pub subject: RecipeSubjectV2,
    pub request: RecipeRequestV2,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecipePlanRequestWire {
    schema_version: u32,
    subject: RecipeSubjectV2,
    request: RecipeRequestV2,
}

impl TryFrom<RecipePlanRequestWire> for RecipePlanRequestV2 {
    type Error = &'static str;

    fn try_from(wire: RecipePlanRequestWire) -> Result<Self, Self::Error> {
        let request = Self {
            schema_version: wire.schema_version,
            subject: wire.subject,
            request: wire.request,
        };
        request.validate_data()?;
        Ok(request)
    }
}

impl RecipePlanRequestV2 {
    pub fn validate(&self) -> Result<(), PlanError> {
        self.validate_data().map_err(|_| PlanError::Unsupported)
    }

    fn validate_data(&self) -> Result<(), &'static str> {
        if self.schema_version != 2 {
            return Err("Recipe schemaVersion must be 2");
        }
        validate_path(&self.request.project_path)?;
        if self.request.binding.scope != BindingScope::Project
            || self.request.binding.strategy != BindingStrategy::Direct
        {
            return Err("Recipe binding must use project/direct");
        }
        validate_tools(self.request.items.iter().map(|item| item.tool_id))?;
        for item in &self.request.items {
            validate_version(item.tool_id, &item.version)?;
            validate_id(&item.snapshot_request_id)?;
            if let RecipeSelectionV2::Reuse { installation_id } = &item.selection {
                validate_id(installation_id)?;
            }
        }
        Ok(())
    }
}

/// Option 字段也必须显式出现，防止 Rust 接受 TS 白名单拒绝的缺项。
fn required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecipeFileEvidenceV2 {
    pub path: String,
    #[serde(deserialize_with = "required_option")]
    pub canonical_path: Option<String>,
    pub exists: bool,
    #[serde(deserialize_with = "required_option")]
    pub sha256: Option<String>,
}

impl RecipeFileEvidenceV2 {
    fn validate(&self) -> Result<(), &'static str> {
        validate_path(&self.path)?;
        if self
            .canonical_path
            .as_ref()
            .is_some_and(|canonical| canonical != &self.path)
        {
            return Err("Recipe file evidence must not alias another path");
        }
        if self.exists != self.sha256.is_some() {
            return Err("Recipe file existence and fingerprint disagree");
        }
        if self.sha256.as_ref().is_some_and(|sha| {
            sha.len() != 64
                || !sha
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        }) {
            return Err("Recipe fingerprint must be lowercase SHA256");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecipeConfigurationDiffV2 {
    pub path: String,
    pub field: String,
    #[serde(deserialize_with = "required_option")]
    pub before: Option<String>,
    #[serde(deserialize_with = "required_option")]
    pub after: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecipeCapabilityV2 {
    pub can_preview: bool,
    pub can_execute: bool,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecipeProviderPackageV2 {
    pub provider_id: ProviderId,
    pub package_id: String,
    pub kind: PackageKind,
    pub platform: Platform,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RecipeTargetV2 {
    /// 来源、版本与配置获批；实例身份必须在实际安装重扫后取得。
    Install {},
    Reuse {
        installation_id: String,
        locator: String,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RecipeDeclarationStateV2 {
    Matched,
    Unconstrained,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecipeDeclarationsV2 {
    /// 传输的声明判定；反序列化只校验格式，不代表已检查实际磁盘。
    pub state: RecipeDeclarationStateV2,
    pub files: Vec<RecipeFileEvidenceV2>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecipePlanItemV2 {
    pub tool_id: InventoryTool,
    pub version: String,
    pub snapshot_request_id: String,
    pub snapshot_generation: String,
    pub provider_package: RecipeProviderPackageV2,
    pub target: RecipeTargetV2,
    pub allowed_configuration_field: String,
    pub declarations: RecipeDeclarationsV2,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecipePlanPayloadV2 {
    pub plan_id: String,
    pub created_at: u64,
    pub expires_at: u64,
    pub project_directory: String,
    pub configuration: RecipeFileEvidenceV2,
    pub configuration_diff: Vec<RecipeConfigurationDiffV2>,
    pub capability: RecipeCapabilityV2,
    pub items: Vec<RecipePlanItemV2>,
    pub steps: Vec<RecipeStepIdentityV2>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", try_from = "RecipeOperationPlanWire")]
pub struct RecipeOperationPlanV2 {
    pub schema_version: u32,
    pub subject: RecipeSubjectV2,
    pub plan: RecipePlanPayloadV2,
    pub step_graph: Vec<StepGraphEntryV2>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecipeOperationPlanWire {
    schema_version: u32,
    subject: RecipeSubjectV2,
    plan: RecipePlanPayloadV2,
    step_graph: Vec<StepGraphEntryV2>,
}

impl TryFrom<RecipeOperationPlanWire> for RecipeOperationPlanV2 {
    type Error = &'static str;

    fn try_from(wire: RecipeOperationPlanWire) -> Result<Self, Self::Error> {
        let plan = Self {
            schema_version: wire.schema_version,
            subject: wire.subject,
            plan: wire.plan,
            step_graph: wire.step_graph,
        };
        plan.validate_data()?;
        Ok(plan)
    }
}

impl RecipeOperationPlanV2 {
    pub fn validate(&self) -> Result<(), PlanError> {
        self.validate_data().map_err(|_| PlanError::Unsupported)
    }

    fn validate_data(&self) -> Result<(), &'static str> {
        if self.schema_version != 2 {
            return Err("Recipe schemaVersion must be 2");
        }
        let plan = &self.plan;
        validate_id(&plan.plan_id)?;
        if plan.created_at > MAX_SAFE_TIMESTAMP
            || plan.expires_at > MAX_SAFE_TIMESTAMP
            || plan.expires_at <= plan.created_at
            || plan.expires_at - plan.created_at > super::TTL_MS
        {
            return Err("invalid Recipe approval timestamps");
        }
        validate_path(&plan.project_directory)?;
        plan.configuration.validate()?;
        let allowed_configs = [
            format!("{}/mise.toml", plan.project_directory),
            format!("{}/.mise.toml", plan.project_directory),
        ];
        if !allowed_configs.contains(&plan.configuration.path) {
            return Err("Recipe configuration must be inside its frozen project");
        }
        if !plan.capability.can_preview
            || plan.capability.can_execute
            || plan
                .capability
                .reason
                .trim_matches(|ch: char| ch.is_whitespace() || ch == '\u{feff}')
                .is_empty()
            || !bounded_text(&plan.capability.reason, MAX_REASON_BYTES)
        {
            return Err("Recipe capability must explicitly remain preview-only");
        }
        let tools = validate_tools(plan.items.iter().map(|item| item.tool_id))?;
        for item in &plan.items {
            validate_version(item.tool_id, &item.version)?;
            validate_id(&item.snapshot_request_id)?;
            validate_generation(&item.snapshot_generation)?;
            let tool = tool_name(item.tool_id);
            if item.provider_package.provider_id != ProviderId::Mise
                || item.provider_package.package_id != tool
                || item.provider_package.kind != PackageKind::Runtime
                || item.provider_package.platform != Platform::Macos
                || item.allowed_configuration_field != format!("tools.{tool}")
            {
                return Err("Recipe item must freeze native mise/runtime/macOS and its field");
            }
            if let RecipeTargetV2::Reuse {
                installation_id,
                locator,
            } = &item.target
            {
                validate_id(installation_id)?;
                validate_path(locator)?;
            }
            if item.declarations.files.len() > MAX_DECLARATION_FILES {
                return Err("Recipe declaration evidence exceeds its file budget");
            }
            let mut paths = BTreeSet::new();
            let project_prefix = format!("{}/", plan.project_directory);
            for file in &item.declarations.files {
                file.validate()?;
                if !file.path.starts_with(&project_prefix) || !paths.insert(&file.path) {
                    return Err("Recipe declarations must be unique frozen project files");
                }
            }
        }
        if plan.configuration_diff.len() > plan.items.len() {
            return Err("Recipe configuration diff exceeds the approved item budget");
        }
        let mut fields = BTreeSet::new();
        for diff in &plan.configuration_diff {
            let item = plan
                .items
                .iter()
                .find(|item| item.allowed_configuration_field == diff.field)
                .ok_or("Recipe diff contains an unapproved field")?;
            if diff.path != plan.configuration.path
                || !fields.insert(&diff.field)
                || diff.after.as_deref() != Some(item.version.as_str())
                || diff
                    .before
                    .as_ref()
                    .is_some_and(|before| !bounded_text(before, MAX_DIFF_VALUE_BYTES))
            {
                return Err("Recipe diff must select only its approved exact versions");
            }
        }
        validate_recipe_steps(&tools, &plan.steps)?;
        if self.step_graph.len() != plan.steps.len() {
            return Err("Recipe graph and fixed steps differ in length");
        }
        for (index, node) in self.step_graph.iter().enumerate() {
            let expected_dependencies = if index == 0 {
                vec![]
            } else {
                vec![format!("{}-step-{}", plan.plan_id, index - 1)]
            };
            if node.step_id != format!("{}-step-{index}", plan.plan_id)
                || node.step_index != index
                || node.depends_on != expected_dependencies
            {
                return Err("Recipe graph must exactly project its fixed linear template");
            }
        }
        Ok(())
    }
}

fn validate_tools(
    tools: impl Iterator<Item = InventoryTool>,
) -> Result<Vec<InventoryTool>, &'static str> {
    let tools: Vec<_> = tools.collect();
    if tools.is_empty() || tools.len() > MAX_RECIPE_ITEMS {
        return Err("Recipe must contain one or two tools");
    }
    if tools.iter().collect::<BTreeSet<_>>().len() != tools.len() {
        return Err("Recipe must not repeat a tool, even with the same version");
    }
    Ok(tools)
}

fn tool_name(tool: InventoryTool) -> &'static str {
    match tool {
        InventoryTool::Node => "node",
        InventoryTool::Python => "python",
    }
}

fn validate_id(id: &str) -> Result<(), &'static str> {
    if super::valid_id(id) {
        Ok(())
    } else {
        Err("Recipe ID must be 1..128 ASCII letters, numbers, hyphens or underscores")
    }
}

fn validate_path(path: &str) -> Result<(), &'static str> {
    if path.len() > MAX_PATH_BYTES
        || !path.starts_with('/')
        || path == "/"
        || path.contains('\\')
        || path.chars().any(char::is_control)
        || path
            .split('/')
            .skip(1)
            .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
    {
        return Err("Recipe path must be a bounded absolute path without ambiguous segments");
    }
    Ok(())
}

fn bounded_text(value: &str, budget: usize) -> bool {
    value.len() <= budget && !value.chars().any(char::is_control)
}

fn validate_version(tool: InventoryTool, version: &str) -> Result<(), &'static str> {
    let accepted = match tool {
        InventoryTool::Node => super::exact_version(version),
        InventoryTool::Python => crate::providers::python::stable(version),
    };
    if accepted {
        Ok(())
    } else {
        Err("Recipe requires an exact supported stable three-part version")
    }
}

fn validate_generation(generation: &str) -> Result<(), &'static str> {
    if generation.is_empty()
        || generation.len() > 20
        || (generation.len() > 1 && generation.starts_with('0'))
        || !generation.bytes().all(|byte| byte.is_ascii_digit())
        || generation.parse::<u64>().is_err()
    {
        return Err("Recipe snapshot generation must be canonical decimal u64");
    }
    Ok(())
}

impl OperationPlanService {
    pub fn preview_any_v2_current(
        &self,
        query: &InventoryQueryService,
        request: AnyPlanRequestV2,
    ) -> Result<OperationPlanV2, PlanError> {
        self.preview_any_v2_using(request, |request| self.preview_v2_current(query, request))
    }

    fn preview_any_v2_using(
        &self,
        request: AnyPlanRequestV2,
        preview_tool: impl FnOnce(PlanRequestV2) -> Result<OperationPlanV2, PlanError>,
    ) -> Result<OperationPlanV2, PlanError> {
        request.validate()?;
        match request {
            AnyPlanRequestV2::Tool(request) => preview_tool(request),
            AnyPlanRequestV2::Recipe(_) => Err(PlanError::Disabled),
        }
    }
}

#[cfg(test)]
#[path = "recipe_contract_tests.rs"]
mod tests;
