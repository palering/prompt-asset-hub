use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::{DomainError, RecordTimestamps, TenantId, common::required};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TenantStatus {
    #[default]
    Active,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tenant {
    pub id: TenantId,
    pub slug: String,
    pub name: String,
    pub status: TenantStatus,
    pub timestamps: RecordTimestamps,
}

impl Tenant {
    pub fn new(
        id: TenantId,
        slug: impl Into<String>,
        name: impl Into<String>,
        now: OffsetDateTime,
    ) -> Result<Self, DomainError> {
        let slug = required("tenant.slug", slug)?;
        if !slug
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        {
            return Err(DomainError::InvalidValue {
                field: "tenant.slug",
                reason: "only ASCII letters, digits, '-' and '_' are allowed".into(),
            });
        }

        Ok(Self {
            id,
            slug,
            name: required("tenant.name", name)?,
            status: TenantStatus::Active,
            timestamps: RecordTimestamps::new(now),
        })
    }

    #[must_use]
    pub fn local(id: TenantId, now: OffsetDateTime) -> Self {
        Self {
            id,
            slug: "local".into(),
            name: "Local Library".into(),
            status: TenantStatus::Active,
            timestamps: RecordTimestamps::new(now),
        }
    }
}
