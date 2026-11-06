//! Recipe 记录仅定义严格契约和内存恢复；不会写入当前 v1 journal 或启动执行。
use super::{CompensationOutcome, ExecutionRecord, ExecutionStatus, PlanError};
use crate::inventory::query::InventoryTool;
use serde::{Deserialize, Deserializer, Serialize};

pub const MAX_RECIPE_ITEMS: usize = 2;
pub const MAX_RECIPE_STEPS: usize = 24;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum RecipeSubjectV2 {
    Recipe {},
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RecipePhase {
    Preflight,
    Select,
    InstallPreflight,
    InstallCommand,
    InstallRefresh,
    InstallVerify,
    BindPreflight,
    BindCommand,
    BindRefresh,
    BindVerify,
    Compensate,
    Reconcile,
    Finalize,
}

const ITEM_PHASES: [RecipePhase; 11] = [
    RecipePhase::Select,
    RecipePhase::InstallPreflight,
    RecipePhase::InstallCommand,
    RecipePhase::InstallRefresh,
    RecipePhase::InstallVerify,
    RecipePhase::BindPreflight,
    RecipePhase::BindCommand,
    RecipePhase::BindRefresh,
    RecipePhase::BindVerify,
    RecipePhase::Compensate,
    RecipePhase::Reconcile,
];

fn required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecipeStepIdentityV2 {
    pub index: u32,
    #[serde(deserialize_with = "required_option")]
    pub item_index: Option<u32>,
    pub phase: RecipePhase,
}

pub fn recipe_step_template(
    tools: &[InventoryTool],
) -> Result<Vec<RecipeStepIdentityV2>, &'static str> {
    if tools.is_empty() || tools.len() > MAX_RECIPE_ITEMS {
        return Err("Recipe requires one or two tool items");
    }
    if tools
        .iter()
        .enumerate()
        .any(|(index, tool)| tools[..index].contains(tool))
    {
        return Err("Recipe tool items must be unique");
    }
    let mut steps = vec![RecipeStepIdentityV2 {
        index: 0,
        item_index: None,
        phase: RecipePhase::Preflight,
    }];
    for item_index in 0..tools.len() {
        for phase in ITEM_PHASES {
            steps.push(RecipeStepIdentityV2 {
                index: steps.len() as u32,
                item_index: Some(item_index as u32),
                phase,
            });
        }
    }
    steps.push(RecipeStepIdentityV2 {
        index: steps.len() as u32,
        item_index: None,
        phase: RecipePhase::Finalize,
    });
    Ok(steps)
}

pub fn validate_recipe_steps(
    tools: &[InventoryTool],
    steps: &[RecipeStepIdentityV2],
) -> Result<(), &'static str> {
    if steps.len() > MAX_RECIPE_STEPS || steps != recipe_step_template(tools)? {
        return Err("Recipe steps must match the fixed item template");
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RecipeStepStatus {
    Planned,
    Running,
    Completed,
    Failed,
    Skipped,
    Interrupted,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecipeExecutionStepV2 {
    pub index: u32,
    #[serde(deserialize_with = "required_option")]
    pub item_index: Option<u32>,
    pub phase: RecipePhase,
    pub status: RecipeStepStatus,
}
impl RecipeExecutionStepV2 {
    pub fn identity(&self) -> RecipeStepIdentityV2 {
        RecipeStepIdentityV2 {
            index: self.index,
            item_index: self.item_index,
            phase: self.phase,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RecipeItemStatus {
    Planned,
    Running,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RecipeItemOutcome {
    Satisfied,
    Configured,
    StepFailed,
    Cancelled,
    Interrupted,
    CompensationConflict,
    ScanIncomplete,
    PreconditionChanged,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecipeExecutionItemV2 {
    pub tool_id: InventoryTool,
    pub status: RecipeItemStatus,
    #[serde(deserialize_with = "required_option")]
    pub outcome: Option<RecipeItemOutcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compensation: Option<CompensationOutcome>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RecipeOutcome {
    RecipeCompleted,
    StepFailed,
    Cancelled,
    Interrupted,
    CompensationConflict,
    ScanIncomplete,
    PreconditionChanged,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeExecutionRecordV2 {
    pub schema_version: u32,
    pub subject: RecipeSubjectV2,
    pub id: String,
    pub status: ExecutionStatus,
    pub created_at: u64,
    pub updated_at: u64,
    pub items: Vec<RecipeExecutionItemV2>,
    pub steps: Vec<RecipeExecutionStepV2>,
    pub outcome: Option<RecipeOutcome>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecordWire {
    schema_version: u32,
    subject: RecipeSubjectV2,
    id: String,
    status: ExecutionStatus,
    created_at: u64,
    updated_at: u64,
    items: Vec<RecipeExecutionItemV2>,
    steps: Vec<RecipeExecutionStepV2>,
    #[serde(deserialize_with = "required_option")]
    outcome: Option<RecipeOutcome>,
}

impl<'de> Deserialize<'de> for RecipeExecutionRecordV2 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = RecordWire::deserialize(deserializer)?;
        let record = Self {
            schema_version: wire.schema_version,
            subject: wire.subject,
            id: wire.id,
            status: wire.status,
            created_at: wire.created_at,
            updated_at: wire.updated_at,
            items: wire.items,
            steps: wire.steps,
            outcome: wire.outcome,
        };
        record.validate().map_err(serde::de::Error::custom)?;
        Ok(record)
    }
}

impl RecipeExecutionRecordV2 {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != 2
            || !super::valid_id(&self.id)
            || self.created_at > MAX_SAFE_INTEGER
            || self.updated_at > MAX_SAFE_INTEGER
            || self.updated_at < self.created_at
        {
            return Err("Invalid Recipe record version, identity or timestamps");
        }
        let tools: Vec<_> = self.items.iter().map(|item| item.tool_id).collect();
        let identities: Vec<_> = self
            .steps
            .iter()
            .map(RecipeExecutionStepV2::identity)
            .collect();
        validate_recipe_steps(&tools, &identities)?;
        let terminal = self.status != ExecutionStatus::Running;
        if self
            .steps
            .iter()
            .filter(|step| step.status == RecipeStepStatus::Running)
            .count()
            > 1
            || self
                .items
                .iter()
                .filter(|item| item.status == RecipeItemStatus::Running)
                .count()
                > 1
            || (terminal
                && self.steps.iter().any(|step| {
                    matches!(
                        step.status,
                        RecipeStepStatus::Planned | RecipeStepStatus::Running
                    )
                }))
        {
            return Err("Recipe parent and phase states are inconsistent");
        }
        match self.status {
            ExecutionStatus::Running if self.outcome.is_none() => {}
            ExecutionStatus::Completed if self.outcome == Some(RecipeOutcome::RecipeCompleted) => {
                if self
                    .items
                    .iter()
                    .any(|item| item.status != RecipeItemStatus::Completed)
                    || self.steps.iter().any(|step| {
                        !matches!(
                            step.status,
                            RecipeStepStatus::Completed | RecipeStepStatus::Skipped
                        )
                    })
                    || self
                        .steps
                        .first()
                        .is_none_or(|step| step.status != RecipeStepStatus::Completed)
                    || self
                        .steps
                        .last()
                        .is_none_or(|step| step.status != RecipeStepStatus::Completed)
                {
                    return Err("Completed Recipe must have completed items and finalization");
                }
            }
            ExecutionStatus::Interrupted if self.outcome == Some(RecipeOutcome::Interrupted) => {}
            ExecutionStatus::Failed | ExecutionStatus::Partial
                if self.outcome.is_some_and(|outcome| {
                    !matches!(
                        outcome,
                        RecipeOutcome::RecipeCompleted
                            | RecipeOutcome::Interrupted
                            | RecipeOutcome::Cancelled
                    )
                }) => {}
            ExecutionStatus::Cancelled if self.outcome == Some(RecipeOutcome::Cancelled) => {}
            _ => return Err("Recipe parent status and outcome are inconsistent"),
        }
        for (index, item) in self.items.iter().enumerate() {
            let steps: Vec<_> = self
                .steps
                .iter()
                .filter(|step| step.item_index == Some(index as u32))
                .collect();
            let successful = matches!(
                item.outcome,
                Some(RecipeItemOutcome::Satisfied | RecipeItemOutcome::Configured)
            );
            match item.status {
                RecipeItemStatus::Planned
                    if item.outcome.is_none() && item.compensation.is_none() =>
                {
                    if steps.iter().any(|step| {
                        !matches!(
                            step.status,
                            RecipeStepStatus::Planned | RecipeStepStatus::Skipped
                        )
                    }) {
                        return Err("Unstarted Recipe item cannot contain executed phases");
                    }
                }
                RecipeItemStatus::Running
                    if self.status == ExecutionStatus::Running && item.outcome.is_none() => {}
                RecipeItemStatus::Completed if successful && item.compensation.is_none() => {
                    if steps.iter().any(|step| {
                        !matches!(
                            step.status,
                            RecipeStepStatus::Completed | RecipeStepStatus::Skipped
                        )
                    }) || steps
                        .first()
                        .is_none_or(|step| step.status != RecipeStepStatus::Completed)
                        || steps
                            .last()
                            .is_none_or(|step| step.status != RecipeStepStatus::Completed)
                    {
                        return Err("Completed Recipe item requires selection and reconciliation");
                    }
                    let phase_status = |phase| {
                        steps
                            .iter()
                            .find(|step| step.phase == phase)
                            .map(|step| step.status)
                    };
                    if phase_status(RecipePhase::Compensate) != Some(RecipeStepStatus::Skipped) {
                        return Err("Successful Recipe item cannot have been compensated");
                    }
                    match item.outcome {
                        Some(RecipeItemOutcome::Configured) => {
                            if [
                                RecipePhase::BindPreflight,
                                RecipePhase::BindCommand,
                                RecipePhase::BindRefresh,
                                RecipePhase::BindVerify,
                            ]
                            .iter()
                            .any(|phase| phase_status(*phase) != Some(RecipeStepStatus::Completed))
                            {
                                return Err("Configured Recipe item requires all binding phases");
                            }
                        }
                        Some(RecipeItemOutcome::Satisfied) => {
                            if phase_status(RecipePhase::BindCommand)
                                != Some(RecipeStepStatus::Skipped)
                                || phase_status(RecipePhase::BindVerify)
                                    != Some(RecipeStepStatus::Completed)
                            {
                                return Err("Satisfied Recipe item requires no write and actual binding verification");
                            }
                        }
                        _ => return Err("Successful Recipe item requires a success outcome"),
                    }
                }
                RecipeItemStatus::Interrupted
                    if item.outcome == Some(RecipeItemOutcome::Interrupted) => {}
                RecipeItemStatus::Failed
                    if item.outcome.is_some()
                        && !successful
                        && !matches!(
                            item.outcome,
                            Some(RecipeItemOutcome::Interrupted | RecipeItemOutcome::Cancelled)
                        ) => {}
                RecipeItemStatus::Cancelled
                    if item.outcome.is_some()
                        && !successful
                        && item.outcome != Some(RecipeItemOutcome::Interrupted) => {}
                _ => return Err("Recipe item status and outcome are inconsistent"),
            }
            if matches!(
                item.status,
                RecipeItemStatus::Completed
                    | RecipeItemStatus::Failed
                    | RecipeItemStatus::Cancelled
                    | RecipeItemStatus::Interrupted
            ) && steps.iter().any(|step| {
                matches!(
                    step.status,
                    RecipeStepStatus::Planned | RecipeStepStatus::Running
                )
            }) {
                return Err("Terminal Recipe item cannot contain unfinished phases");
            }
            let compensation_phase = steps
                .iter()
                .find(|step| step.phase == RecipePhase::Compensate)
                .ok_or("Missing compensation phase")?;
            match (compensation_phase.status, item.compensation) {
                (RecipeStepStatus::Completed, Some(CompensationOutcome::Completed))
                | (
                    RecipeStepStatus::Failed,
                    Some(CompensationOutcome::Failed | CompensationOutcome::Conflict),
                )
                | (
                    RecipeStepStatus::Planned
                    | RecipeStepStatus::Running
                    | RecipeStepStatus::Skipped
                    | RecipeStepStatus::Interrupted,
                    None,
                ) => {}
                _ => return Err("Recipe compensation result must match its phase"),
            }
        }
        for step in &self.steps {
            if step.status == RecipeStepStatus::Running
                && step.item_index.is_some_and(|index| {
                    self.items[index as usize].status != RecipeItemStatus::Running
                })
            {
                return Err("Running Recipe phase requires its running item");
            }
        }
        Ok(())
    }

    pub fn interrupt_in_memory(&mut self, updated_at: u64) -> Result<bool, &'static str> {
        if updated_at > MAX_SAFE_INTEGER {
            return Err("Recipe recovery timestamp exceeds the safe integer budget");
        }
        self.validate()?;
        if self.status != ExecutionStatus::Running {
            return Ok(false);
        }
        self.status = ExecutionStatus::Interrupted;
        self.outcome = Some(RecipeOutcome::Interrupted);
        self.updated_at = updated_at.max(self.updated_at);
        for item in &mut self.items {
            if item.status == RecipeItemStatus::Running {
                item.status = RecipeItemStatus::Interrupted;
                item.outcome = Some(RecipeItemOutcome::Interrupted);
            }
        }
        for step in &mut self.steps {
            step.status = match step.status {
                RecipeStepStatus::Running => RecipeStepStatus::Interrupted,
                RecipeStepStatus::Planned => RecipeStepStatus::Skipped,
                status => status,
            };
        }
        self.validate()?;
        Ok(true)
    }
}

#[derive(Clone, Debug)]
pub enum ExecutionRecordEnvelope {
    Tool(ExecutionRecord),
    Recipe(RecipeExecutionRecordV2),
}

/// Version dispatch happens before parsing the body. A malformed new record has no legacy fallback.
pub fn decode_execution_record(data: &[u8]) -> Result<ExecutionRecordEnvelope, PlanError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Header {
        schema_version: u32,
    }
    if data.len() > super::journal::MAX_BYTES as usize {
        return Err(PlanError::Journal);
    }
    let header: Header = serde_json::from_slice(data).map_err(|_| PlanError::Journal)?;
    match header.schema_version {
        1 => {
            let record: ExecutionRecord =
                serde_json::from_slice(data).map_err(|_| PlanError::Journal)?;
            super::journal::validate_v1_record(&record)?;
            Ok(ExecutionRecordEnvelope::Tool(record))
        }
        2 => serde_json::from_slice(data)
            .map(ExecutionRecordEnvelope::Recipe)
            .map_err(|_| PlanError::Journal),
        _ => Err(PlanError::Journal),
    }
}

pub fn encode_execution_record(record: &ExecutionRecordEnvelope) -> Result<Vec<u8>, PlanError> {
    let data = match record {
        ExecutionRecordEnvelope::Tool(record) => {
            super::journal::validate_v1_record(record)?;
            serde_json::to_vec(record)
        }
        ExecutionRecordEnvelope::Recipe(record) => {
            record.validate().map_err(|_| PlanError::Journal)?;
            serde_json::to_vec(record)
        }
    }
    .map_err(|_| PlanError::Journal)?;
    if data.len() > super::journal::MAX_BYTES as usize {
        return Err(PlanError::Journal);
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn fixture() -> Value {
        serde_json::from_str(include_str!(
            "../../../fixtures/operations/recipe-record-v2.json"
        ))
        .unwrap()
    }

    fn running_record() -> RecipeExecutionRecordV2 {
        let mut value = fixture()["records"]["interrupted"].clone();
        value["status"] = json!("running");
        value["outcome"] = Value::Null;
        value["updatedAt"] = json!(120);
        value["items"][1]["status"] = json!("running");
        value["items"][1]["outcome"] = Value::Null;
        for step in value["steps"].as_array_mut().unwrap() {
            let index = step["index"].as_u64().unwrap();
            if index == 18 {
                step["status"] = json!("running");
            } else if index > 18 {
                step["status"] = json!("planned");
            }
        }
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn fixed_template_rejects_extra_missing_reordered_and_unowned_phases() {
        let one = recipe_step_template(&[InventoryTool::Python]).unwrap();
        assert_eq!(one.len(), 13);
        let tools = [InventoryTool::Python, InventoryTool::Node];
        let two = recipe_step_template(&tools).unwrap();
        assert_eq!(two.len(), MAX_RECIPE_STEPS);
        assert_eq!(two[12].item_index, Some(1));
        assert_eq!(two[23].item_index, None);
        validate_recipe_steps(&tools, &two).unwrap();
        assert!(recipe_step_template(&[]).is_err());
        assert!(recipe_step_template(&[InventoryTool::Node, InventoryTool::Node]).is_err());
        for mutate in [0, 1, 2, 3] {
            let mut steps = two.clone();
            match mutate {
                0 => steps.push(two[23]),
                1 => {
                    steps.pop();
                }
                2 => steps.swap(2, 3),
                _ => steps[1].item_index = None,
            }
            assert!(validate_recipe_steps(&tools, &steps).is_err());
        }
        assert!(serde_json::from_value::<RecipeStepIdentityV2>(
            json!({"index":0,"phase":"preflight"})
        )
        .is_err());
        assert!(serde_json::from_value::<RecipeSubjectV2>(
            json!({"kind":"recipe","toolId":"node"})
        )
        .is_err());
    }

    #[test]
    fn shared_recipe_records_roundtrip_and_invalid_new_records_never_fallback() {
        let fixture = fixture();
        for value in fixture["records"].as_object().unwrap().values() {
            let record: RecipeExecutionRecordV2 = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(serde_json::to_value(&record).unwrap(), *value);
            let decoded = decode_execution_record(&serde_json::to_vec(value).unwrap()).unwrap();
            assert!(matches!(decoded, ExecutionRecordEnvelope::Recipe(_)));
        }
        for invalid in fixture["invalidRecords"].as_array().unwrap() {
            let value = super::super::fixture_cases::materialize(&fixture["records"], invalid);
            assert!(
                serde_json::from_value::<RecipeExecutionRecordV2>(value.clone()).is_err(),
                "{}",
                invalid["name"]
            );
            assert!(
                decode_execution_record(&serde_json::to_vec(&value).unwrap()).is_err(),
                "{}",
                invalid["name"]
            );
        }
    }

    #[test]
    fn decoder_keeps_v1_node_compatibility_and_rejects_duplicate_or_unknown_versions() {
        let legacy = json!({"schemaVersion":1,"id":"legacy","action":"verify","status":"completed","createdAt":0,"updatedAt":0,"steps":[],"outcome":"verified"});
        let decoded = decode_execution_record(&serde_json::to_vec(&legacy).unwrap()).unwrap();
        let ExecutionRecordEnvelope::Tool(record) = decoded else {
            panic!("Legacy record must remain Tool")
        };
        assert_eq!(record.tool_id, InventoryTool::Node);
        assert_eq!(serde_json::to_value(record).unwrap(), legacy);
        for version in [2, 3] {
            let mut invalid = legacy.clone();
            invalid["schemaVersion"] = json!(version);
            assert!(decode_execution_record(&serde_json::to_vec(&invalid).unwrap()).is_err());
        }
        let duplicate = br#"{"schemaVersion":1,"schemaVersion":2,"id":"legacy"}"#;
        assert!(decode_execution_record(duplicate).is_err());
        assert!(decode_execution_record(&vec![b' '; 256 * 1024 + 1]).is_err());
    }

    #[test]
    fn legacy_clock_order_and_missing_outcome_remain_v1_compatible() {
        for value in fixture()["legacyRecords"].as_object().unwrap().values() {
            let decoded = decode_execution_record(&serde_json::to_vec(value).unwrap()).unwrap();
            let ExecutionRecordEnvelope::Tool(record) = decoded else {
                panic!("Legacy record must remain Tool")
            };
            assert_eq!(record.tool_id, InventoryTool::Node);
            let mut canonical = value.clone();
            if canonical.get("outcome").is_none() {
                canonical["outcome"] = Value::Null;
            }
            assert_eq!(serde_json::to_value(&record).unwrap(), canonical);
            let encoded = encode_execution_record(&ExecutionRecordEnvelope::Tool(record)).unwrap();
            assert_eq!(
                serde_json::from_slice::<Value>(&encoded).unwrap(),
                canonical
            );
        }
    }

    #[test]
    fn encoder_validates_programmatic_records_and_roundtrips_both_versions() {
        let fixture = fixture();
        let legacy = json!({"schemaVersion":1,"id":"legacy","action":"verify","status":"completed","createdAt":0,"updatedAt":0,"steps":[],"outcome":"verified"});
        for value in [&legacy, &fixture["records"]["partial"]] {
            let decoded = decode_execution_record(&serde_json::to_vec(value).unwrap()).unwrap();
            let encoded = encode_execution_record(&decoded).unwrap();
            assert_eq!(serde_json::from_slice::<Value>(&encoded).unwrap(), *value);
            decode_execution_record(&encoded).unwrap();
        }
        let mut recipe = running_record();
        recipe.schema_version = 3;
        assert!(encode_execution_record(&ExecutionRecordEnvelope::Recipe(recipe)).is_err());
        let ExecutionRecordEnvelope::Tool(mut tool) =
            decode_execution_record(&serde_json::to_vec(&legacy).unwrap()).unwrap()
        else {
            panic!("Expected Tool")
        };
        tool.schema_version = 2;
        assert!(encode_execution_record(&ExecutionRecordEnvelope::Tool(tool)).is_err());
        let mut unverified: RecipeExecutionRecordV2 =
            serde_json::from_value(fixture["records"]["completedConfigured"].clone()).unwrap();
        unverified.steps[9].status = RecipeStepStatus::Skipped;
        assert!(encode_execution_record(&ExecutionRecordEnvelope::Recipe(unverified)).is_err());
    }

    #[test]
    fn compensation_phase_and_summary_match_in_both_directions() {
        let mut record: RecipeExecutionRecordV2 =
            serde_json::from_value(fixture()["records"]["partial"].clone()).unwrap();
        for outcome in [CompensationOutcome::Failed, CompensationOutcome::Conflict] {
            record.steps[21].status = RecipeStepStatus::Failed;
            record.items[1].compensation = Some(outcome);
            record.items[1].outcome = Some(if outcome == CompensationOutcome::Conflict {
                RecipeItemOutcome::CompensationConflict
            } else {
                RecipeItemOutcome::StepFailed
            });
            record.outcome = Some(if outcome == CompensationOutcome::Conflict {
                RecipeOutcome::CompensationConflict
            } else {
                RecipeOutcome::StepFailed
            });
            record.validate().unwrap();
        }
        record.items[1].compensation = None;
        assert!(record.validate().is_err());
        record.steps[21].status = RecipeStepStatus::Completed;
        record.items[1].compensation = Some(CompensationOutcome::Completed);
        record.validate().unwrap();
        record.items[1].compensation = None;
        assert!(record.validate().is_err());
        record.steps[21].status = RecipeStepStatus::Skipped;
        record.validate().unwrap();
    }

    #[test]
    fn in_memory_recovery_preserves_completed_items_and_is_idempotent() {
        let mut record = running_record();
        let completed = record.items[0].clone();
        assert!(record.interrupt_in_memory(130).unwrap());
        assert_eq!(record.items[0], completed);
        assert_eq!(
            serde_json::to_value(&record).unwrap(),
            fixture()["records"]["interrupted"]
        );
        let before = serde_json::to_value(&record).unwrap();
        assert!(!record.interrupt_in_memory(140).unwrap());
        assert_eq!(serde_json::to_value(record).unwrap(), before);
    }

    #[test]
    fn recovery_rejects_unsafe_time_or_invalid_records_before_mutation() {
        let mut record = running_record();
        let before = serde_json::to_value(&record).unwrap();
        assert!(record.interrupt_in_memory(MAX_SAFE_INTEGER + 1).is_err());
        assert_eq!(serde_json::to_value(&record).unwrap(), before);
        record.steps[1].item_index = None;
        let invalid = serde_json::to_value(&record).unwrap();
        assert!(record.interrupt_in_memory(130).is_err());
        assert_eq!(serde_json::to_value(record).unwrap(), invalid);
    }
}
