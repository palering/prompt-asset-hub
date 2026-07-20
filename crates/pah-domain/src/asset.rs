use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::{
    AssetFileId, AssetId, AssetLinkId, DomainError, GenerationRunId, PromptId, RecordTimestamps,
    TenantId,
    common::{optional, required},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaType {
    Image,
    Video,
    Reference,
    Frame,
    Document,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetFileRole {
    Original,
    Thumbnail,
    Preview,
    Poster,
    Proxy,
    Frame,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetFile {
    pub id: AssetFileId,
    pub tenant_id: TenantId,
    pub asset_id: AssetId,
    pub role: AssetFileRole,
    pub storage_key: String,
    pub mime_type: String,
    pub byte_size: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub sha256: String,
    pub generator_version: Option<String>,
    pub created_at: OffsetDateTime,
}

impl AssetFile {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: AssetFileId,
        tenant_id: TenantId,
        asset_id: AssetId,
        role: AssetFileRole,
        storage_key: impl Into<String>,
        mime_type: impl Into<String>,
        byte_size: u64,
        width: Option<u32>,
        height: Option<u32>,
        sha256: impl Into<String>,
        generator_version: Option<String>,
        now: OffsetDateTime,
    ) -> Result<Self, DomainError> {
        if byte_size == 0 {
            return Err(DomainError::InvalidValue {
                field: "asset_file.byte_size",
                reason: "asset files cannot be empty".into(),
            });
        }
        validate_dimensions(width, height)?;
        let storage_key = match AssetLocation::stored(storage_key)? {
            AssetLocation::Stored { storage_key } => storage_key,
            AssetLocation::External { .. } => unreachable!(),
        };
        Ok(Self {
            id,
            tenant_id,
            asset_id,
            role,
            storage_key,
            mime_type: required("asset_file.mime_type", mime_type)?,
            byte_size,
            width,
            height,
            sha256: validate_sha256(&sha256.into())?,
            generator_version: optional(generator_version),
            created_at: now,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AssetStatus {
    Processing,
    #[default]
    Ready,
    Failed,
    Archived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AssetVisibility {
    #[default]
    Private,
    Unlisted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AssetLocation {
    Stored { storage_key: String },
    External { url: String },
}

impl AssetLocation {
    pub fn stored(storage_key: impl Into<String>) -> Result<Self, DomainError> {
        let storage_key = required("asset.storage_key", storage_key)?;
        if storage_key.starts_with('/') || storage_key.contains("..") {
            return Err(DomainError::InvalidValue {
                field: "asset.storage_key",
                reason: "must be a relative, traversal-free object key".into(),
            });
        }
        Ok(Self::Stored { storage_key })
    }

    pub fn external(url: impl Into<String>) -> Result<Self, DomainError> {
        let url = required("asset.external_url", url)?;
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err(DomainError::InvalidValue {
                field: "asset.external_url",
                reason: "only http and https URLs are supported".into(),
            });
        }
        Ok(Self::External { url })
    }

    #[must_use]
    pub fn storage_key(&self) -> Option<&str> {
        match self {
            Self::Stored { storage_key } => Some(storage_key),
            Self::External { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    pub id: AssetId,
    pub tenant_id: TenantId,
    pub media_type: MediaType,
    pub location: AssetLocation,
    pub original_filename: Option<String>,
    pub mime_type: Option<String>,
    pub byte_size: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<u64>,
    pub sha256: Option<String>,
    pub thumbnail_key: Option<String>,
    pub status: AssetStatus,
    pub visibility: AssetVisibility,
    pub timestamps: RecordTimestamps,
}

impl Asset {
    #[allow(clippy::too_many_arguments)]
    pub fn new_stored(
        id: AssetId,
        tenant_id: TenantId,
        media_type: MediaType,
        storage_key: impl Into<String>,
        original_filename: Option<String>,
        mime_type: impl Into<String>,
        byte_size: u64,
        width: Option<u32>,
        height: Option<u32>,
        duration_ms: Option<u64>,
        sha256: impl Into<String>,
        now: OffsetDateTime,
    ) -> Result<Self, DomainError> {
        if byte_size == 0 {
            return Err(DomainError::InvalidValue {
                field: "asset.byte_size",
                reason: "stored blobs cannot be empty".into(),
            });
        }
        validate_dimensions(width, height)?;
        validate_duration(media_type, duration_ms)?;
        let sha256 = validate_sha256(&sha256.into())?;

        Ok(Self {
            id,
            tenant_id,
            media_type,
            location: AssetLocation::stored(storage_key)?,
            original_filename: optional(original_filename),
            mime_type: Some(required("asset.mime_type", mime_type)?),
            byte_size: Some(byte_size),
            width,
            height,
            duration_ms,
            sha256: Some(sha256),
            thumbnail_key: None,
            status: AssetStatus::Ready,
            visibility: AssetVisibility::Private,
            timestamps: RecordTimestamps::new(now),
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_external(
        id: AssetId,
        tenant_id: TenantId,
        media_type: MediaType,
        url: impl Into<String>,
        original_filename: Option<String>,
        mime_type: Option<String>,
        width: Option<u32>,
        height: Option<u32>,
        duration_ms: Option<u64>,
        now: OffsetDateTime,
    ) -> Result<Self, DomainError> {
        validate_dimensions(width, height)?;
        validate_duration(media_type, duration_ms)?;
        Ok(Self {
            id,
            tenant_id,
            media_type,
            location: AssetLocation::external(url)?,
            original_filename: optional(original_filename),
            mime_type: optional(mime_type),
            byte_size: None,
            width,
            height,
            duration_ms,
            sha256: None,
            thumbnail_key: None,
            status: AssetStatus::Ready,
            visibility: AssetVisibility::Private,
            timestamps: RecordTimestamps::new(now),
        })
    }

    pub fn set_thumbnail_key(&mut self, storage_key: impl Into<String>) -> Result<(), DomainError> {
        let location = AssetLocation::stored(storage_key)?;
        self.thumbnail_key = location.storage_key().map(str::to_owned);
        Ok(())
    }
}

fn validate_dimensions(width: Option<u32>, height: Option<u32>) -> Result<(), DomainError> {
    match (width, height) {
        (Some(0), _) | (_, Some(0)) => Err(DomainError::InvalidValue {
            field: "asset.dimensions",
            reason: "width and height must be greater than zero".into(),
        }),
        (Some(_), None) | (None, Some(_)) => Err(DomainError::InvalidValue {
            field: "asset.dimensions",
            reason: "width and height must be supplied together".into(),
        }),
        _ => Ok(()),
    }
}

fn validate_duration(media_type: MediaType, duration_ms: Option<u64>) -> Result<(), DomainError> {
    if duration_ms == Some(0) {
        return Err(DomainError::InvalidValue {
            field: "asset.duration_ms",
            reason: "duration must be greater than zero".into(),
        });
    }
    if duration_ms.is_some() && media_type != MediaType::Video {
        return Err(DomainError::InvalidValue {
            field: "asset.duration_ms",
            reason: "duration is only valid for video assets".into(),
        });
    }
    Ok(())
}

fn validate_sha256(value: &str) -> Result<String, DomainError> {
    let value = value.trim().to_ascii_lowercase();
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(DomainError::InvalidValue {
            field: "asset.sha256",
            reason: "must be a 64-character hexadecimal digest".into(),
        });
    }
    Ok(value)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetRelationType {
    Output,
    ReferenceInput,
    Inspiration,
    FirstFrame,
    LastFrame,
    DerivedFrom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetLink {
    pub id: AssetLinkId,
    pub tenant_id: TenantId,
    pub asset_id: AssetId,
    pub generation_run_id: Option<GenerationRunId>,
    pub prompt_id: Option<PromptId>,
    pub related_asset_id: Option<AssetId>,
    pub relation_type: AssetRelationType,
    pub sort_order: u32,
    pub note: Option<String>,
    pub created_at: OffsetDateTime,
}

impl AssetLink {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: AssetLinkId,
        tenant_id: TenantId,
        asset_id: AssetId,
        generation_run_id: Option<GenerationRunId>,
        prompt_id: Option<PromptId>,
        related_asset_id: Option<AssetId>,
        relation_type: AssetRelationType,
        sort_order: u32,
        note: Option<String>,
        now: OffsetDateTime,
    ) -> Result<Self, DomainError> {
        if generation_run_id.is_none() && prompt_id.is_none() && related_asset_id.is_none() {
            return Err(DomainError::MissingAssetLinkContext);
        }
        if relation_type == AssetRelationType::DerivedFrom && related_asset_id.is_none() {
            return Err(DomainError::InvalidValue {
                field: "asset_link.related_asset_id",
                reason: "derived_from links require a source asset".into(),
            });
        }
        Ok(Self {
            id,
            tenant_id,
            asset_id,
            generation_run_id,
            prompt_id,
            related_asset_id,
            relation_type,
            sort_order,
            note: optional(note),
            created_at: now,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_assets_reject_unsafe_keys_and_bad_hashes() {
        let result = Asset::new_stored(
            AssetId::new(),
            TenantId::new(),
            MediaType::Image,
            "../outside.png",
            None,
            "image/png",
            10,
            Some(1),
            Some(1),
            None,
            "bad-hash",
            OffsetDateTime::UNIX_EPOCH,
        );
        assert!(result.is_err());
    }

    #[test]
    fn derived_links_require_a_source_asset() {
        let result = AssetLink::new(
            AssetLinkId::new(),
            TenantId::new(),
            AssetId::new(),
            Some(GenerationRunId::new()),
            None,
            None,
            AssetRelationType::DerivedFrom,
            0,
            None,
            OffsetDateTime::UNIX_EPOCH,
        );
        assert!(matches!(result, Err(DomainError::InvalidValue { .. })));
    }
}
