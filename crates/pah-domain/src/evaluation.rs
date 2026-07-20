use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::{
    AssetId, DomainError, EvaluationId, GenerationRunId, RecordTimestamps, TenantId,
    common::optional,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evaluation {
    pub id: EvaluationId,
    pub tenant_id: TenantId,
    pub asset_id: AssetId,
    pub generation_run_id: Option<GenerationRunId>,
    pub overall_score: u8,
    pub is_featured: bool,
    pub is_reusable: bool,
    pub dimension_scores: BTreeMap<String, u8>,
    pub strengths: Option<String>,
    pub issues: Option<String>,
    pub lesson: Option<String>,
    pub created_by: String,
    pub timestamps: RecordTimestamps,
}

impl Evaluation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: EvaluationId,
        tenant_id: TenantId,
        asset_id: AssetId,
        generation_run_id: Option<GenerationRunId>,
        overall_score: u8,
        is_featured: bool,
        is_reusable: bool,
        dimension_scores: BTreeMap<String, u8>,
        strengths: Option<String>,
        issues: Option<String>,
        lesson: Option<String>,
        created_by: impl Into<String>,
        now: OffsetDateTime,
    ) -> Result<Self, DomainError> {
        validate_score("overall", overall_score)?;
        for (dimension, score) in &dimension_scores {
            if dimension.trim().is_empty() {
                return Err(DomainError::EmptyField {
                    field: "evaluation.dimension",
                });
            }
            validate_score(dimension, *score)?;
        }
        let created_by = created_by.into().trim().to_owned();
        if created_by.is_empty() {
            return Err(DomainError::EmptyField {
                field: "evaluation.created_by",
            });
        }

        Ok(Self {
            id,
            tenant_id,
            asset_id,
            generation_run_id,
            overall_score,
            is_featured,
            is_reusable,
            dimension_scores,
            strengths: optional(strengths),
            issues: optional(issues),
            lesson: optional(lesson),
            created_by,
            timestamps: RecordTimestamps::new(now),
        })
    }
}

fn validate_score(field: impl Into<String>, value: u8) -> Result<(), DomainError> {
    let field = field.into();
    if (1..=5).contains(&value) {
        Ok(())
    } else {
        Err(DomainError::ScoreOutOfRange { field, value })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_evaluation_score_uses_the_same_scale() {
        let mut dimensions = BTreeMap::new();
        dimensions.insert("composition".into(), 6);
        let result = Evaluation::new(
            EvaluationId::new(),
            TenantId::new(),
            AssetId::new(),
            None,
            5,
            true,
            true,
            dimensions,
            None,
            None,
            None,
            "local-user",
            OffsetDateTime::UNIX_EPOCH,
        );
        assert!(matches!(
            result,
            Err(DomainError::ScoreOutOfRange { value: 6, .. })
        ));
    }
}
