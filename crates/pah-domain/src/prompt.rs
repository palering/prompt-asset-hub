use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::{
    DomainError, JsonObject, ProjectId, PromptId, PromptVariantId, RecordTimestamps, TenantId,
    common::{optional, required},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PromptStatus {
    #[default]
    Draft,
    Verified,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prompt {
    pub id: PromptId,
    pub tenant_id: TenantId,
    pub project_id: ProjectId,
    pub title: String,
    pub creative_intent: String,
    pub base_prompt: String,
    pub negative_constraints: Option<String>,
    pub status: PromptStatus,
    pub current_version: u32,
    pub created_by: String,
    pub timestamps: RecordTimestamps,
}

impl Prompt {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: PromptId,
        tenant_id: TenantId,
        project_id: ProjectId,
        title: impl Into<String>,
        creative_intent: impl Into<String>,
        base_prompt: impl Into<String>,
        negative_constraints: Option<String>,
        created_by: impl Into<String>,
        now: OffsetDateTime,
    ) -> Result<Self, DomainError> {
        Ok(Self {
            id,
            tenant_id,
            project_id,
            title: required("prompt.title", title)?,
            creative_intent: required("prompt.creative_intent", creative_intent)?,
            base_prompt: required("prompt.base_prompt", base_prompt)?,
            negative_constraints: optional(negative_constraints),
            status: PromptStatus::Draft,
            current_version: 1,
            created_by: required("prompt.created_by", created_by)?,
            timestamps: RecordTimestamps::new(now),
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn revise(
        &mut self,
        title: impl Into<String>,
        creative_intent: impl Into<String>,
        base_prompt: impl Into<String>,
        negative_constraints: Option<String>,
        now: OffsetDateTime,
    ) -> Result<(), DomainError> {
        self.title = required("prompt.title", title)?;
        self.creative_intent = required("prompt.creative_intent", creative_intent)?;
        self.base_prompt = required("prompt.base_prompt", base_prompt)?;
        self.negative_constraints = optional(negative_constraints);
        self.current_version =
            self.current_version
                .checked_add(1)
                .ok_or_else(|| DomainError::InvalidValue {
                    field: "prompt.current_version",
                    reason: "version overflow".into(),
                })?;
        self.status = PromptStatus::Draft;
        self.timestamps.touch(now)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptVariant {
    pub id: PromptVariantId,
    pub tenant_id: TenantId,
    pub prompt_id: PromptId,
    pub provider: String,
    pub model: String,
    pub model_version: Option<String>,
    pub prompt_text: String,
    pub parameters: JsonObject,
    pub adaptation_notes: Option<String>,
    pub success_tips: Option<String>,
    pub failure_patterns: Option<String>,
    pub status: PromptStatus,
    pub revision: u32,
    pub timestamps: RecordTimestamps,
}

impl PromptVariant {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: PromptVariantId,
        tenant_id: TenantId,
        prompt_id: PromptId,
        provider: impl Into<String>,
        model: impl Into<String>,
        model_version: Option<String>,
        prompt_text: impl Into<String>,
        parameters: JsonObject,
        adaptation_notes: Option<String>,
        success_tips: Option<String>,
        failure_patterns: Option<String>,
        now: OffsetDateTime,
    ) -> Result<Self, DomainError> {
        Ok(Self {
            id,
            tenant_id,
            prompt_id,
            provider: required("prompt_variant.provider", provider)?,
            model: required("prompt_variant.model", model)?,
            model_version: optional(model_version),
            prompt_text: required("prompt_variant.prompt_text", prompt_text)?,
            parameters,
            adaptation_notes: optional(adaptation_notes),
            success_tips: optional(success_tips),
            failure_patterns: optional(failure_patterns),
            status: PromptStatus::Draft,
            revision: 1,
            timestamps: RecordTimestamps::new(now),
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn revise(
        &mut self,
        prompt_text: impl Into<String>,
        parameters: JsonObject,
        adaptation_notes: Option<String>,
        success_tips: Option<String>,
        failure_patterns: Option<String>,
        now: OffsetDateTime,
    ) -> Result<(), DomainError> {
        self.prompt_text = required("prompt_variant.prompt_text", prompt_text)?;
        self.parameters = parameters;
        self.adaptation_notes = optional(adaptation_notes);
        self.success_tips = optional(success_tips);
        self.failure_patterns = optional(failure_patterns);
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| DomainError::InvalidValue {
                field: "prompt_variant.revision",
                reason: "revision overflow".into(),
            })?;
        // Editing invalidates the previous verification until the new revision is tested.
        self.status = PromptStatus::Draft;
        self.timestamps.touch(now)
    }

    pub fn mark_verified(&mut self, now: OffsetDateTime) -> Result<(), DomainError> {
        self.status = PromptStatus::Verified;
        self.timestamps.touch(now)
    }
}

#[cfg(test)]
mod tests {
    use time::Duration;

    use super::*;

    #[test]
    fn revising_a_verified_variant_increments_revision_and_returns_to_draft() {
        let now = OffsetDateTime::UNIX_EPOCH;
        let mut variant = PromptVariant::new(
            PromptVariantId::new(),
            TenantId::new(),
            PromptId::new(),
            "openai",
            "gpt-image",
            None,
            "a studio portrait",
            JsonObject::new(),
            None,
            None,
            None,
            now,
        )
        .unwrap();

        variant.mark_verified(now + Duration::seconds(1)).unwrap();
        variant
            .revise(
                "a cinematic studio portrait",
                JsonObject::new(),
                None,
                None,
                None,
                now + Duration::seconds(2),
            )
            .unwrap();

        assert_eq!(variant.revision, 2);
        assert_eq!(variant.status, PromptStatus::Draft);
    }
}
