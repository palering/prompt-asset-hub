use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::{
    AssetId, DomainError, GenerationRunId, JsonObject, PromptVariantId, RecordTimestamps, TenantId,
    common::{optional, required},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationSource {
    Manual,
    Extension,
    Api,
    Agent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GenerationStatus {
    #[default]
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl GenerationStatus {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputSnapshot {
    pub prompt_text: String,
    pub reference_asset_ids: Vec<AssetId>,
    pub extra: JsonObject,
}

impl InputSnapshot {
    pub fn new(
        prompt_text: impl Into<String>,
        reference_asset_ids: Vec<AssetId>,
        extra: JsonObject,
    ) -> Result<Self, DomainError> {
        Ok(Self {
            prompt_text: required("generation.input_snapshot.prompt_text", prompt_text)?,
            reference_asset_ids,
            extra,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenerationCost {
    pub amount_micros: u64,
    pub currency: String,
}

impl GenerationCost {
    pub fn new(amount_micros: u64, currency: impl Into<String>) -> Result<Self, DomainError> {
        let currency = required("generation.cost.currency", currency)?.to_ascii_uppercase();
        if currency.len() != 3 || !currency.bytes().all(|byte| byte.is_ascii_alphabetic()) {
            return Err(DomainError::InvalidValue {
                field: "generation.cost.currency",
                reason: "must be a three-letter currency code".into(),
            });
        }
        Ok(Self {
            amount_micros,
            currency,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerationRun {
    pub id: GenerationRunId,
    pub tenant_id: TenantId,
    pub variant_id: PromptVariantId,
    pub source: GenerationSource,
    pub input_snapshot: InputSnapshot,
    pub parameters_snapshot: JsonObject,
    pub external_url: Option<String>,
    pub started_at: Option<OffsetDateTime>,
    pub completed_at: Option<OffsetDateTime>,
    pub status: GenerationStatus,
    pub failure_message: Option<String>,
    pub cost: Option<GenerationCost>,
    pub created_by: String,
    pub timestamps: RecordTimestamps,
}

impl GenerationRun {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: GenerationRunId,
        tenant_id: TenantId,
        variant_id: PromptVariantId,
        source: GenerationSource,
        input_snapshot: InputSnapshot,
        parameters_snapshot: JsonObject,
        external_url: Option<String>,
        cost: Option<GenerationCost>,
        created_by: impl Into<String>,
        now: OffsetDateTime,
    ) -> Result<Self, DomainError> {
        Ok(Self {
            id,
            tenant_id,
            variant_id,
            source,
            input_snapshot,
            parameters_snapshot,
            external_url: optional(external_url),
            started_at: None,
            completed_at: None,
            status: GenerationStatus::Pending,
            failure_message: None,
            cost,
            created_by: required("generation.created_by", created_by)?,
            timestamps: RecordTimestamps::new(now),
        })
    }

    pub fn start(&mut self, now: OffsetDateTime) -> Result<(), DomainError> {
        self.transition(GenerationStatus::Running)?;
        self.started_at = Some(now);
        self.timestamps.touch(now)
    }

    pub fn succeed(&mut self, now: OffsetDateTime) -> Result<(), DomainError> {
        self.ensure_not_before_start(now)?;
        self.transition(GenerationStatus::Succeeded)?;
        self.completed_at = Some(now);
        self.failure_message = None;
        self.timestamps.touch(now)
    }

    pub fn fail(
        &mut self,
        message: impl Into<String>,
        now: OffsetDateTime,
    ) -> Result<(), DomainError> {
        self.ensure_not_before_start(now)?;
        let message = required("generation.failure_message", message)?;
        self.transition(GenerationStatus::Failed)?;
        self.completed_at = Some(now);
        self.failure_message = Some(message);
        self.timestamps.touch(now)
    }

    pub fn cancel(&mut self, now: OffsetDateTime) -> Result<(), DomainError> {
        self.ensure_not_before_start(now)?;
        self.transition(GenerationStatus::Cancelled)?;
        self.completed_at = Some(now);
        self.timestamps.touch(now)
    }

    fn transition(&mut self, next: GenerationStatus) -> Result<(), DomainError> {
        let allowed = matches!(
            (self.status, next),
            (
                GenerationStatus::Pending,
                GenerationStatus::Running
                    | GenerationStatus::Succeeded
                    | GenerationStatus::Failed
                    | GenerationStatus::Cancelled,
            ) | (
                GenerationStatus::Running,
                GenerationStatus::Succeeded
                    | GenerationStatus::Failed
                    | GenerationStatus::Cancelled,
            )
        );
        if !allowed {
            return Err(DomainError::InvalidStateTransition {
                entity: "generation_run",
                from: format!("{:?}", self.status).to_ascii_lowercase(),
                to: format!("{next:?}").to_ascii_lowercase(),
            });
        }
        self.status = next;
        Ok(())
    }

    fn ensure_not_before_start(&self, value: OffsetDateTime) -> Result<(), DomainError> {
        if self.started_at.is_some_and(|started_at| value < started_at) {
            Err(DomainError::InvalidTimestampOrder {
                field: "completed_at",
                earlier_field: "started_at",
            })
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use time::Duration;

    use super::*;

    fn run(now: OffsetDateTime) -> GenerationRun {
        GenerationRun::new(
            GenerationRunId::new(),
            TenantId::new(),
            PromptVariantId::new(),
            GenerationSource::Manual,
            InputSnapshot::new("actual prompt", Vec::new(), JsonObject::new()).unwrap(),
            JsonObject::new(),
            None,
            None,
            "local-user",
            now,
        )
        .unwrap()
    }

    #[test]
    fn generation_runs_are_terminal_after_success() {
        let now = OffsetDateTime::UNIX_EPOCH;
        let mut run = run(now);
        run.start(now + Duration::seconds(1)).unwrap();
        run.succeed(now + Duration::seconds(2)).unwrap();

        assert_eq!(run.status, GenerationStatus::Succeeded);
        assert!(run.status.is_terminal());
        assert!(matches!(
            run.fail("late failure", now + Duration::seconds(3)),
            Err(DomainError::InvalidStateTransition { .. })
        ));
    }
}
