//! Bounded structured operational event schema.

use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::diagnostics::redaction::{
    sanitize_stable_identifier, sanitize_telemetry_field, sanitize_telemetry_field_key,
    telemetry_field_redaction,
};

use super::context::{CorrelationContext, MAX_COMPONENT_NAME_LEN};
use super::privacy::{EventScope, FieldSensitivity};

pub const MAX_EVENT_ATTRIBUTES: usize = 16;
pub const MAX_ATTRIBUTE_STRING_LEN: usize = 256;
pub const MAX_ATTRIBUTE_KEY_LEN: usize = 48;
pub const MAX_EVENT_NAME_LEN: usize = 80;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EventSeverity {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OutcomeClass {
    Success,
    Cancelled,
    Rejected,
    Timeout,
    Unavailable,
    InvalidInput,
    RateLimited,
    Conflict,
    InternalFailure,
    DegradedFallback,
}

impl OutcomeClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Cancelled => "cancelled",
            Self::Rejected => "rejected",
            Self::Timeout => "timeout",
            Self::Unavailable => "unavailable",
            Self::InvalidInput => "invalid_input",
            Self::RateLimited => "rate_limited",
            Self::Conflict => "conflict",
            Self::InternalFailure => "internal_failure",
            Self::DegradedFallback => "degraded_fallback",
        }
    }

    pub const fn is_success_like(self) -> bool {
        matches!(self, Self::Success | Self::DegradedFallback)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AttributeValue {
    Bool(bool),
    Signed(i64),
    Unsigned(u64),
    Float(f64),
    Text(String),
    Duration(Duration),
    Stable(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum SafeAttributeValue {
    Bool(bool),
    Signed(i64),
    Unsigned(u64),
    Float(f64),
    Text(String),
    DurationMillis(u64),
    Stable(String),
}

impl SafeAttributeValue {
    fn estimated_size_bytes(&self) -> usize {
        match self {
            Self::Text(value) | Self::Stable(value) => value.len().saturating_add(24),
            Self::Bool(_) => 1,
            Self::Signed(_) | Self::Unsigned(_) | Self::Float(_) | Self::DurationMillis(_) => 8,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EventAttribute {
    key: String,
    value: SafeAttributeValue,
    sensitivity: FieldSensitivity,
}

impl EventAttribute {
    pub fn try_new(
        key: impl AsRef<str>,
        value: AttributeValue,
        sensitivity: FieldSensitivity,
    ) -> Result<Self, AttributeError> {
        if matches!(
            sensitivity,
            FieldSensitivity::Sensitive | FieldSensitivity::Secret
        ) {
            return Err(AttributeError::SensitivityRejected);
        }

        let key = sanitize_telemetry_field_key(key.as_ref(), MAX_ATTRIBUTE_KEY_LEN)
            .ok_or(AttributeError::InvalidKey)?;
        let value = sanitize_attribute_value(&key, value)?;
        Ok(Self {
            key,
            value,
            sensitivity,
        })
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn value(&self) -> &SafeAttributeValue {
        &self.value
    }

    pub const fn sensitivity(&self) -> FieldSensitivity {
        self.sensitivity
    }

    fn estimated_size_bytes(&self) -> usize {
        const ATTRIBUTE_OVERHEAD: usize = 32;
        self.key
            .len()
            .saturating_add(self.value.estimated_size_bytes())
            .saturating_add(ATTRIBUTE_OVERHEAD)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttributeError {
    InvalidKey,
    InvalidValue,
    SensitivityRejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttributeInsertStatus {
    Added,
    DroppedLimit,
    DroppedInvalid,
    DroppedSensitivity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventBuildError {
    InvalidEventName,
    InvalidComponent,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OperationalEvent {
    name: String,
    timestamp: DateTime<Utc>,
    severity: EventSeverity,
    component: String,
    context: CorrelationContext,
    outcome: OutcomeClass,
    scope: EventScope,
    attributes: Vec<EventAttribute>,
    dropped_attributes: u32,
}

impl OperationalEvent {
    pub fn new(
        name: impl AsRef<str>,
        component: impl AsRef<str>,
        severity: EventSeverity,
        outcome: OutcomeClass,
        scope: EventScope,
    ) -> Result<Self, EventBuildError> {
        let name = sanitize_stable_identifier(name.as_ref(), MAX_EVENT_NAME_LEN)
            .ok_or(EventBuildError::InvalidEventName)?;
        let component = sanitize_stable_identifier(component.as_ref(), MAX_COMPONENT_NAME_LEN)
            .ok_or(EventBuildError::InvalidComponent)?;

        Ok(Self {
            name,
            timestamp: Utc::now(),
            severity,
            component,
            context: CorrelationContext::empty(),
            outcome,
            scope,
            attributes: Vec::with_capacity(MAX_EVENT_ATTRIBUTES),
            dropped_attributes: 0,
        })
    }

    pub fn at(mut self, timestamp: DateTime<Utc>) -> Self {
        self.timestamp = timestamp;
        self
    }

    pub fn with_context(mut self, context: CorrelationContext) -> Self {
        self.context = context;
        self
    }

    pub fn attribute(
        mut self,
        key: impl AsRef<str>,
        value: AttributeValue,
        sensitivity: FieldSensitivity,
    ) -> Self {
        let _ = self.push_attribute(key, value, sensitivity);
        self
    }

    pub fn push_attribute(
        &mut self,
        key: impl AsRef<str>,
        value: AttributeValue,
        sensitivity: FieldSensitivity,
    ) -> AttributeInsertStatus {
        if self.attributes.len() >= MAX_EVENT_ATTRIBUTES {
            self.dropped_attributes = self.dropped_attributes.saturating_add(1);
            return AttributeInsertStatus::DroppedLimit;
        }

        match EventAttribute::try_new(key, value, sensitivity) {
            Ok(attribute) => {
                self.attributes.push(attribute);
                AttributeInsertStatus::Added
            }
            Err(AttributeError::SensitivityRejected) => {
                self.dropped_attributes = self.dropped_attributes.saturating_add(1);
                AttributeInsertStatus::DroppedSensitivity
            }
            Err(AttributeError::InvalidKey | AttributeError::InvalidValue) => {
                self.dropped_attributes = self.dropped_attributes.saturating_add(1);
                AttributeInsertStatus::DroppedInvalid
            }
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn timestamp(&self) -> DateTime<Utc> {
        self.timestamp
    }

    pub const fn severity(&self) -> EventSeverity {
        self.severity
    }

    pub fn component(&self) -> &str {
        &self.component
    }

    pub fn context(&self) -> &CorrelationContext {
        &self.context
    }

    pub const fn outcome(&self) -> OutcomeClass {
        self.outcome
    }

    pub const fn scope(&self) -> EventScope {
        self.scope
    }

    pub fn attributes(&self) -> &[EventAttribute] {
        &self.attributes
    }

    pub const fn dropped_attributes(&self) -> u32 {
        self.dropped_attributes
    }

    /// Conservative encoded-size estimate used for bounded in-memory queues.
    /// It intentionally over-counts small scalar overhead rather than relying
    /// on allocator capacity, which is implementation-specific.
    pub fn estimated_size_bytes(&self) -> usize {
        const FIXED_OVERHEAD: usize = 192;
        self.name
            .len()
            .saturating_add(self.component.len())
            .saturating_add(self.context.estimated_size_bytes())
            .saturating_add(
                self.attributes
                    .iter()
                    .map(EventAttribute::estimated_size_bytes)
                    .fold(0usize, usize::saturating_add),
            )
            .saturating_add(FIXED_OVERHEAD)
    }

    pub(crate) const fn is_critical(&self) -> bool {
        matches!(self.severity, EventSeverity::Warn | EventSeverity::Error)
    }
}

fn sanitize_attribute_value(
    key: &str,
    value: AttributeValue,
) -> Result<SafeAttributeValue, AttributeError> {
    if let Some(redacted) = telemetry_field_redaction(key) {
        return Ok(SafeAttributeValue::Text(redacted.to_string()));
    }

    match value {
        AttributeValue::Bool(value) => Ok(SafeAttributeValue::Bool(value)),
        AttributeValue::Signed(value) => Ok(SafeAttributeValue::Signed(value)),
        AttributeValue::Unsigned(value) => Ok(SafeAttributeValue::Unsigned(value)),
        AttributeValue::Float(value) if value.is_finite() => Ok(SafeAttributeValue::Float(value)),
        AttributeValue::Float(_) => Err(AttributeError::InvalidValue),
        AttributeValue::Text(value) => Ok(SafeAttributeValue::Text(
            sanitize_telemetry_field(key, &value, MAX_ATTRIBUTE_STRING_LEN).into_string(),
        )),
        AttributeValue::Duration(value) => Ok(SafeAttributeValue::DurationMillis(
            value.as_millis().min(u128::from(u64::MAX)) as u64,
        )),
        AttributeValue::Stable(value) => {
            sanitize_stable_identifier(&value, MAX_ATTRIBUTE_STRING_LEN)
                .map(SafeAttributeValue::Stable)
                .ok_or(AttributeError::InvalidValue)
        }
    }
}
