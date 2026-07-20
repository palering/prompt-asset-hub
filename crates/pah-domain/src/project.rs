use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::{
    AssetId, DomainError, ProjectId, RecordTimestamps, TenantId,
    common::{optional, required},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    #[default]
    Active,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: ProjectId,
    pub tenant_id: TenantId,
    pub name: String,
    pub description: Option<String>,
    pub cover_asset_id: Option<AssetId>,
    pub status: ProjectStatus,
    pub timestamps: RecordTimestamps,
}

impl Project {
    pub fn new(
        id: ProjectId,
        tenant_id: TenantId,
        name: impl Into<String>,
        description: Option<String>,
        now: OffsetDateTime,
    ) -> Result<Self, DomainError> {
        Ok(Self {
            id,
            tenant_id,
            name: required("project.name", name)?,
            description: optional(description),
            cover_asset_id: None,
            status: ProjectStatus::Active,
            timestamps: RecordTimestamps::new(now),
        })
    }

    pub fn archive(&mut self, now: OffsetDateTime) -> Result<(), DomainError> {
        self.status = ProjectStatus::Archived;
        self.timestamps.touch(now)
    }
}
