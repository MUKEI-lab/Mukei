//! Privacy-safe, bounded and sink-neutral operational observability.
//!
//! This module is local-first: it does not create a network client or remote
//! exporter. Callers explicitly own an [`ObservabilityRecorder`], and any
//! future sink is attached through a bounded failure-isolated queue.

mod clock;
mod context;
mod event;
mod health;
mod metrics;
mod privacy;
mod recorder;
mod sink;
mod slo;

pub use clock::{ObservabilityClock, SystemClock};
pub use context::{ContextDimensions, CorrelationContext};
pub use event::{
    AttributeError, AttributeInsertStatus, AttributeValue, EventAttribute, EventBuildError,
    EventSeverity, OperationalEvent, OutcomeClass, SafeAttributeValue, MAX_ATTRIBUTE_KEY_LEN,
    MAX_ATTRIBUTE_STRING_LEN, MAX_EVENT_ATTRIBUTES, MAX_EVENT_NAME_LEN,
};
pub use health::{
    HealthPublishStatus, HealthRegistry, HealthSignal, HealthSignalError, HealthSignalSnapshot,
    HealthState, HealthSummary,
};
pub use metrics::{
    BucketSnapshot, DistributionSnapshot, DistributionSpec, DistributionSpecError,
    MetricDimensions, MetricKey, MetricKind, MetricRecordStatus, MetricRegistry,
    MetricRegistrySnapshot, MetricSeriesSnapshot, MetricValueSnapshot,
};
pub use privacy::{EventScope, FieldSensitivity, TelemetryPolicy, TelemetryPrivacyMode};
pub use recorder::{
    DiagnosticSnapshot, EventRecordStatus, ObservabilityConfig, ObservabilityMemoryLimits,
    ObservabilityRecorder, ProcessMetadata, RecorderStatsSnapshot,
};
pub use sink::{
    DiagnosticSink, SinkError, SinkHealthState, SinkInstallError, SinkQueueLimits,
    SinkStatsSnapshot,
};
pub use slo::{
    SloDimensions, SloIndicatorKind, SloObservation, SloRecordStatus, SloRegistry,
    SloRegistrySnapshot, SloSampleState, SloSeriesSnapshot, SloSummarySnapshot,
    MIN_SLO_OPERATION_SAMPLES,
};

#[cfg(test)]
mod tests;
