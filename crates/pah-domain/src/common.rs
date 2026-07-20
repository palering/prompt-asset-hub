use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

use crate::DomainError;

/// JSON-shaped model parameters with deterministic key ordering.
pub type JsonObject = BTreeMap<String, Value>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordTimestamps {
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub deleted_at: Option<OffsetDateTime>,
}

impl RecordTimestamps {
    #[must_use]
    pub const fn new(now: OffsetDateTime) -> Self {
        Self {
            created_at: now,
            updated_at: now,
            deleted_at: None,
        }
    }

    pub fn touch(&mut self, now: OffsetDateTime) -> Result<(), DomainError> {
        if now < self.created_at {
            return Err(DomainError::InvalidTimestampOrder {
                field: "updated_at",
                earlier_field: "created_at",
            });
        }
        self.updated_at = now;
        Ok(())
    }

    pub fn soft_delete(&mut self, now: OffsetDateTime) -> Result<(), DomainError> {
        self.touch(now)?;
        self.deleted_at = Some(now);
        Ok(())
    }

    pub fn restore(&mut self, now: OffsetDateTime) -> Result<(), DomainError> {
        self.touch(now)?;
        self.deleted_at = None;
        Ok(())
    }
}

pub(crate) fn required(
    field: &'static str,
    value: impl Into<String>,
) -> Result<String, DomainError> {
    let value = value.into().trim().to_owned();
    if value.is_empty() {
        Err(DomainError::EmptyField { field })
    } else {
        Ok(value)
    }
}

pub(crate) fn optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim().to_owned();
        (!value.is_empty()).then_some(value)
    })
}
