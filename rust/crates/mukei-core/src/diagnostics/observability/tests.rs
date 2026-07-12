use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use chrono::{DateTime, Duration as ChronoDuration, Utc};

use super::*;

#[derive(Debug)]
struct FakeClock {
    monotonic: Mutex<Duration>,
    wall: Mutex<DateTime<Utc>>,
}

impl FakeClock {
    fn new() -> Self {
        Self {
            monotonic: Mutex::new(Duration::ZERO),
            wall: Mutex::new(Utc::now()),
        }
    }

    fn advance(&self, duration: Duration) {
        let mut now = self.monotonic.lock().unwrap();
        *now = now.saturating_add(duration);
    }

    fn rollback_wall(&self, duration: ChronoDuration) {
        let mut wall = self.wall.lock().unwrap();
        *wall = wall.clone() - duration;
    }
}

impl ObservabilityClock for FakeClock {
    fn monotonic_now(&self) -> Duration {
        *self.monotonic.lock().unwrap()
    }

    fn wall_now(&self) -> DateTime<Utc> {
        self.wall.lock().unwrap().clone()
    }
}

fn event(name: &str, severity: EventSeverity) -> OperationalEvent {
    OperationalEvent::new(
        name,
        "diagnostics",
        severity,
        OutcomeClass::Success,
        EventScope::Essential,
    )
    .unwrap()
}

fn exported_config() -> ObservabilityConfig {
    ObservabilityConfig {
        policy: TelemetryPolicy::essential_with_export(),
        ..ObservabilityConfig::default()
    }
}

#[test]
fn event_queue_is_bounded_by_bytes_and_count() {
    let limits = ObservabilityMemoryLimits {
        event_buffer_bytes: 1_024,
        max_single_event_bytes: 700,
        critical_event_reserve_bytes: 128,
        ..ObservabilityMemoryLimits::default()
    };
    let recorder = ObservabilityRecorder::with_limits(ObservabilityConfig::default(), limits);

    for index in 0..20 {
        let item = event(&format!("event_{index}"), EventSeverity::Info).attribute(
            "safe_note",
            AttributeValue::Text("x".repeat(220)),
            FieldSensitivity::OperationalSafe,
        );
        let _ = recorder.record_event(item);
        assert!(recorder.stats_snapshot().event_buffer_bytes <= 1_024);
        assert!(recorder.snapshot().recent_events.len() <= 256);
    }
}

#[test]
fn oversized_event_is_rejected_without_queue_growth() {
    let limits = ObservabilityMemoryLimits {
        event_buffer_bytes: 2_048,
        max_single_event_bytes: 260,
        critical_event_reserve_bytes: 0,
        ..ObservabilityMemoryLimits::default()
    };
    let recorder = ObservabilityRecorder::with_limits(ObservabilityConfig::default(), limits);
    let oversized = event("oversized", EventSeverity::Info).attribute(
        "safe_note",
        AttributeValue::Text("x".repeat(200)),
        FieldSensitivity::OperationalSafe,
    );

    assert_eq!(
        recorder.record_event(oversized),
        EventRecordStatus::OversizedRejected
    );
    assert_eq!(recorder.stats_snapshot().event_buffer_bytes, 0);
    assert_eq!(recorder.stats_snapshot().event_oversized_drops, 1);
}


#[test]
fn critical_budget_evicts_noncritical_before_distinct_failures() {
    let limits = ObservabilityMemoryLimits {
        event_buffer_bytes: 600,
        max_single_event_bytes: 400,
        critical_event_reserve_bytes: 250,
        ..ObservabilityMemoryLimits::default()
    };
    let recorder = ObservabilityRecorder::with_limits(ObservabilityConfig::default(), limits);

    assert_eq!(
        recorder.record_event(event("background_info", EventSeverity::Info)),
        EventRecordStatus::Recorded
    );
    assert_eq!(
        recorder.record_event(event("critical_a", EventSeverity::Error)),
        EventRecordStatus::Recorded
    );
    assert_eq!(
        recorder.record_event(event("critical_b", EventSeverity::Error)),
        EventRecordStatus::Recorded
    );

    let names = recorder
        .snapshot()
        .recent_events
        .into_iter()
        .map(|item| item.name().to_string())
        .collect::<Vec<_>>();
    assert!(!names.contains(&"background_info".to_string()));
    assert!(names.contains(&"critical_a".to_string()));
    assert!(names.contains(&"critical_b".to_string()));
    assert!(recorder.stats_snapshot().event_buffer_bytes <= 600);
}

#[test]
fn invalid_metric_identifier_is_rejected_without_series_growth() {
    let registry = MetricRegistry::new(8, Duration::from_secs(60));
    assert_eq!(
        registry.increment_counter(
            "user supplied label with spaces",
            MetricDimensions::new(),
            1,
        ),
        MetricRecordStatus::InvalidIdentity
    );
    assert!(registry.snapshot().series.is_empty());
}

#[test]
fn metric_cardinality_limit_and_label_bound_are_enforced() {
    let config = ObservabilityConfig {
        max_metric_series: 2,
        ..ObservabilityConfig::default()
    };
    let recorder = ObservabilityRecorder::new(config);
    let dimensions = MetricDimensions::new()
        .with_component("core")
        .with_operation_kind("inference")
        .with_backend_kind("local")
        .with_feature_area("chat")
        .with_result_class(OutcomeClass::Success);
    assert_eq!(dimensions.label_count(), 5);

    assert_eq!(
        recorder.increment_counter(EventScope::Essential, "m1", dimensions.clone(), 1),
        MetricRecordStatus::Recorded
    );
    assert_eq!(
        recorder.increment_counter(EventScope::Essential, "m2", dimensions.clone(), 1),
        MetricRecordStatus::Recorded
    );
    assert_eq!(
        recorder.increment_counter(EventScope::Essential, "m3", dimensions, 1),
        MetricRecordStatus::SeriesLimitExceeded
    );
    assert_eq!(recorder.stats_snapshot().metric_cardinality_overflow, 1);
}

#[test]
fn gauges_keep_latest_and_counters_aggregate() {
    let registry = MetricRegistry::new(8, Duration::from_secs(60));
    assert_eq!(
        registry.set_gauge("queue_depth", MetricDimensions::new(), 1.0),
        MetricRecordStatus::Recorded
    );
    assert_eq!(
        registry.set_gauge("queue_depth", MetricDimensions::new(), 7.0),
        MetricRecordStatus::Recorded
    );
    assert_eq!(
        registry.increment_counter("requests", MetricDimensions::new(), 2),
        MetricRecordStatus::Recorded
    );
    assert_eq!(
        registry.increment_counter("requests", MetricDimensions::new(), 3),
        MetricRecordStatus::Recorded
    );

    let snapshot = registry.snapshot();
    let gauge = snapshot
        .series
        .iter()
        .find(|series| series.key.name() == "queue_depth")
        .unwrap();
    assert_eq!(gauge.current, MetricValueSnapshot::Gauge(Some(7.0)));

    let counter = snapshot
        .series
        .iter()
        .find(|series| series.key.name() == "requests")
        .unwrap();
    assert_eq!(counter.current, MetricValueSnapshot::Counter(5));
}

#[test]
fn distinct_critical_events_are_not_coalesced() {
    let recorder = ObservabilityRecorder::default();
    assert_eq!(
        recorder.record_event(event("failure_a", EventSeverity::Error)),
        EventRecordStatus::Recorded
    );
    assert_eq!(
        recorder.record_event(event("failure_b", EventSeverity::Error)),
        EventRecordStatus::Recorded
    );
    let names = recorder
        .snapshot()
        .recent_events
        .into_iter()
        .map(|item| item.name().to_string())
        .collect::<Vec<_>>();
    assert!(names.contains(&"failure_a".to_string()));
    assert!(names.contains(&"failure_b".to_string()));
}

#[test]
fn sensitive_content_is_redacted_before_enqueue() {
    let recorder = ObservabilityRecorder::default();
    let raw = "my private user prompt must never survive";
    let item = event("privacy_test", EventSeverity::Info).attribute(
        "user_prompt",
        AttributeValue::Text(raw.to_string()),
        FieldSensitivity::OperationalSafe,
    );
    assert_eq!(recorder.record_event(item), EventRecordStatus::Recorded);

    let snapshot = recorder.snapshot();
    let attribute = &snapshot.recent_events[0].attributes()[0];
    assert_eq!(
        attribute.value(),
        &SafeAttributeValue::Text("[redacted-content]".to_string())
    );
    assert!(!format!("{snapshot:?}").contains(raw));
}


#[test]
fn sensitive_field_names_are_redacted_independent_of_value_type() {
    let item = event("field_key_redaction", EventSeverity::Info)
        .attribute(
            "authorization_header",
            AttributeValue::Stable("opaquecredential".to_string()),
            FieldSensitivity::OperationalSafe,
        )
        .attribute(
            "device_id",
            AttributeValue::Unsigned(42),
            FieldSensitivity::OperationalSafe,
        )
        .attribute(
            "tenant_id",
            AttributeValue::Stable("tenant_123".to_string()),
            FieldSensitivity::OperationalSafe,
        )
        .attribute(
            "prompt_token_count",
            AttributeValue::Unsigned(128),
            FieldSensitivity::OperationalSafe,
        );

    let values = item
        .attributes()
        .iter()
        .map(|attribute| attribute.value().clone())
        .collect::<Vec<_>>();
    assert_eq!(
        values,
        vec![
            SafeAttributeValue::Text("[redacted-secret]".to_string()),
            SafeAttributeValue::Text("[redacted-identifier]".to_string()),
            SafeAttributeValue::Text("[redacted-identifier]".to_string()),
            SafeAttributeValue::Unsigned(128),
        ]
    );
}

#[test]
fn sensitive_classification_is_rejected_before_enqueue() {
    let mut item = event("sensitivity_test", EventSeverity::Info);
    assert_eq!(
        item.push_attribute(
            "api_key",
            AttributeValue::Text("sk-secret".to_string()),
            FieldSensitivity::Secret,
        ),
        AttributeInsertStatus::DroppedSensitivity
    );
    let recorder = ObservabilityRecorder::default();
    assert_eq!(recorder.record_event(item), EventRecordStatus::Recorded);
    assert!(recorder.snapshot().recent_events[0].attributes().is_empty());
}


struct CapturingSink {
    events: Mutex<Vec<String>>,
}

impl CapturingSink {
    fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
        }
    }
}

impl DiagnosticSink for CapturingSink {
    fn emit_event(
        &self,
        _policy: TelemetryPolicy,
        event: &OperationalEvent,
    ) -> Result<(), SinkError> {
        self.events.lock().unwrap().push(format!("{event:?}"));
        Ok(())
    }
}

#[test]
fn export_envelope_never_exposes_raw_sensitive_field_content() {
    let recorder = ObservabilityRecorder::new(exported_config());
    let sink = Arc::new(CapturingSink::new());
    recorder.add_sink(sink.clone()).unwrap();
    let raw = "private prompt payload should not survive";
    let item = event("export_redaction", EventSeverity::Info).attribute(
        "user_prompt",
        AttributeValue::Text(raw.to_string()),
        FieldSensitivity::OperationalSafe,
    );
    assert_eq!(recorder.record_event(item), EventRecordStatus::Recorded);
    wait_until(|| !sink.events.lock().unwrap().is_empty());
    let emitted = sink.events.lock().unwrap().join("
");
    assert!(!emitted.contains(raw));
    assert!(emitted.contains("[redacted-content]"));
}

struct CountingSink {
    events: AtomicU64,
}

impl CountingSink {
    fn new() -> Self {
        Self {
            events: AtomicU64::new(0),
        }
    }
}

impl DiagnosticSink for CountingSink {
    fn emit_event(
        &self,
        _policy: TelemetryPolicy,
        _event: &OperationalEvent,
    ) -> Result<(), SinkError> {
        self.events.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

struct BlockingSink {
    state: Mutex<bool>,
    ready: Condvar,
    started: AtomicU64,
}

impl BlockingSink {
    fn new() -> Self {
        Self {
            state: Mutex::new(false),
            ready: Condvar::new(),
            started: AtomicU64::new(0),
        }
    }

    fn release(&self) {
        let mut released = self.state.lock().unwrap();
        *released = true;
        self.ready.notify_all();
    }

    fn wait_until_started(&self) {
        wait_until(|| self.started.load(Ordering::Relaxed) > 0);
    }
}

impl DiagnosticSink for BlockingSink {
    fn emit_event(
        &self,
        _policy: TelemetryPolicy,
        _event: &OperationalEvent,
    ) -> Result<(), SinkError> {
        self.started.fetch_add(1, Ordering::Relaxed);
        let mut released = self.state.lock().unwrap();
        while !*released {
            released = self.ready.wait(released).unwrap();
        }
        Ok(())
    }
}


struct BlockingMetricsSink {
    state: Mutex<bool>,
    ready: Condvar,
    started: AtomicU64,
}

impl BlockingMetricsSink {
    fn new() -> Self {
        Self {
            state: Mutex::new(false),
            ready: Condvar::new(),
            started: AtomicU64::new(0),
        }
    }

    fn release(&self) {
        let mut released = self.state.lock().unwrap();
        *released = true;
        self.ready.notify_all();
    }

    fn wait_until_started(&self) {
        wait_until(|| self.started.load(Ordering::Relaxed) > 0);
    }
}

impl DiagnosticSink for BlockingMetricsSink {
    fn emit_event(
        &self,
        _policy: TelemetryPolicy,
        _event: &OperationalEvent,
    ) -> Result<(), SinkError> {
        Ok(())
    }

    fn emit_metrics(
        &self,
        _policy: TelemetryPolicy,
        _snapshot: &MetricRegistrySnapshot,
    ) -> Result<(), SinkError> {
        self.started.fetch_add(1, Ordering::Relaxed);
        let mut released = self.state.lock().unwrap();
        while !*released {
            released = self.ready.wait(released).unwrap();
        }
        Ok(())
    }
}

#[test]
fn default_policy_does_not_export_to_installed_sink() {
    let recorder = ObservabilityRecorder::default();
    let sink = Arc::new(CountingSink::new());
    recorder.add_sink(sink.clone()).unwrap();
    assert_eq!(
        recorder.record_event(event("local_only", EventSeverity::Info)),
        EventRecordStatus::Recorded
    );
    thread::sleep(Duration::from_millis(20));
    assert_eq!(sink.events.load(Ordering::Relaxed), 0);
}

#[test]
fn disabled_policy_blocks_local_recording_and_export() {
    let config = ObservabilityConfig {
        policy: TelemetryPolicy::disabled(),
        ..ObservabilityConfig::default()
    };
    let recorder = ObservabilityRecorder::new(config);
    let sink = Arc::new(CountingSink::new());
    recorder.add_sink(sink.clone()).unwrap();
    assert_eq!(
        recorder.record_event(event("disabled", EventSeverity::Info)),
        EventRecordStatus::PolicyRejected
    );
    assert!(recorder.snapshot().recent_events.is_empty());
    assert_eq!(sink.events.load(Ordering::Relaxed), 0);
}

#[test]
fn privacy_epoch_invalidates_already_queued_old_policy_data() {
    let recorder = ObservabilityRecorder::new(exported_config());
    let sink = Arc::new(BlockingSink::new());
    recorder
        .add_sink_with_limits(
            sink.clone(),
            SinkQueueLimits {
                max_count: 8,
                max_bytes: 8 * 1024,
                max_single_envelope_bytes: 2 * 1024,
                disconnect_after_drops: 8,
                slow_callback_threshold: Duration::from_secs(60),
            },
        )
        .unwrap();

    assert_eq!(
        recorder.record_event(event("in_flight", EventSeverity::Info)),
        EventRecordStatus::Recorded
    );
    sink.wait_until_started();
    assert_eq!(
        recorder.record_event(event("queued_old_epoch", EventSeverity::Info)),
        EventRecordStatus::Recorded
    );

    let old_epoch = recorder.privacy_epoch();
    recorder.set_policy(TelemetryPolicy::essential());
    assert!(recorder.privacy_epoch() > old_epoch);
    sink.release();

    wait_until(|| recorder.stats_snapshot().sink_privacy_epoch_drops > 0);
    assert!(recorder.stats_snapshot().sink_privacy_epoch_drops >= 1);
}


#[test]
fn pending_metric_snapshots_coalesce_for_a_stalled_sink() {
    let recorder = ObservabilityRecorder::new(exported_config());
    let sink = Arc::new(BlockingMetricsSink::new());
    recorder
        .add_sink_with_limits(
            sink.clone(),
            SinkQueueLimits {
                max_count: 2,
                max_bytes: 8 * 1024,
                max_single_envelope_bytes: 4 * 1024,
                disconnect_after_drops: 8,
                slow_callback_threshold: Duration::from_secs(60),
            },
        )
        .unwrap();

    recorder.increment_counter(
        EventScope::Essential,
        "requests",
        MetricDimensions::new(),
        1,
    );
    recorder.dispatch_metric_snapshot();
    sink.wait_until_started();

    for value in 2..=8 {
        recorder.set_gauge(
            EventScope::Essential,
            "queue_depth",
            MetricDimensions::new(),
            value as f64,
        );
        recorder.dispatch_metric_snapshot();
    }

    let stats = recorder.stats_snapshot();
    assert!(stats.sink_coalesced > 0);
    assert!(stats.sink_queued_count <= 2);
    assert!(stats.sink_queued_bytes <= 8 * 1024);
    sink.release();
}

#[test]
fn stalled_sink_cannot_create_unbounded_queue_pressure() {
    let recorder = ObservabilityRecorder::new(exported_config());
    let sink = Arc::new(BlockingSink::new());
    recorder
        .add_sink_with_limits(
            sink.clone(),
            SinkQueueLimits {
                max_count: 2,
                max_bytes: 1_024,
                max_single_envelope_bytes: 512,
                disconnect_after_drops: 3,
                slow_callback_threshold: Duration::from_secs(60),
            },
        )
        .unwrap();

    recorder.record_event(event("blocker", EventSeverity::Info));
    sink.wait_until_started();
    for index in 0..32 {
        let _ = recorder.record_event(event(&format!("queued_{index}"), EventSeverity::Info));
    }

    let stats = recorder.stats_snapshot();
    assert!(stats.sink_queued_count <= 2);
    assert!(stats.sink_queued_bytes <= 1_024);
    assert!(stats.sink_queue_drops > 0 || stats.sink_disconnected_drops > 0);
    assert!(matches!(
        stats.sink_health,
        SinkHealthState::Degraded | SinkHealthState::Disconnected
    ));
    sink.release();
}

#[test]
fn healthy_sink_remains_functional_when_another_sink_stalls() {
    let recorder = ObservabilityRecorder::new(exported_config());
    let blocked = Arc::new(BlockingSink::new());
    let healthy = Arc::new(CountingSink::new());
    let limits = SinkQueueLimits {
        max_count: 2,
        max_bytes: 1_024,
        max_single_envelope_bytes: 512,
        disconnect_after_drops: 3,
        slow_callback_threshold: Duration::from_secs(60),
    };
    recorder.add_sink_with_limits(blocked.clone(), limits).unwrap();
    recorder.add_sink_with_limits(healthy.clone(), limits).unwrap();

    recorder.record_event(event("first", EventSeverity::Info));
    blocked.wait_until_started();
    for index in 0..8 {
        recorder.record_event(event(&format!("fanout_{index}"), EventSeverity::Info));
    }
    wait_until(|| healthy.events.load(Ordering::Relaxed) >= 1);
    assert!(healthy.events.load(Ordering::Relaxed) >= 1);
    blocked.release();
}


#[test]
fn monotonic_elapsed_saturates_instead_of_becoming_negative() {
    assert_eq!(
        super::clock::monotonic_elapsed(Duration::from_secs(1), Duration::from_secs(2)),
        Duration::ZERO
    );
}

#[test]
fn monotonic_slo_rotation_survives_wall_clock_rollback() {
    let clock = Arc::new(FakeClock::new());
    let registry = SloRegistry::with_clock(8, Duration::from_secs(10), clock.clone());
    registry.record_operation(
        SloDimensions::new().with_component("core"),
        OutcomeClass::Success,
        Some(Duration::from_millis(10)),
    );
    clock.advance(Duration::from_secs(11));
    clock.rollback_wall(ChronoDuration::hours(2));

    let snapshot = registry.snapshot();
    assert_eq!(snapshot.series[0].current.total_operations, 0);
    assert_eq!(snapshot.series[0].previous.total_operations, 1);
}

#[test]
fn monotonic_health_expiry_survives_wall_clock_rollback() {
    let clock = Arc::new(FakeClock::new());
    let registry = HealthRegistry::with_clock(8, clock.clone());
    registry.publish(
        HealthSignal::new("backend", HealthState::Healthy, "ready")
            .unwrap()
            .expires_after(Duration::from_secs(5)),
    );
    clock.rollback_wall(ChronoDuration::hours(3));
    clock.advance(Duration::from_secs(6));

    let snapshot = registry.snapshot();
    assert_eq!(snapshot.signals[0].state, HealthState::Unknown);
    assert!(snapshot.signals[0].stale);
}

#[test]
fn slo_ratios_report_insufficient_data_until_minimum_sample_rule_is_met() {
    let registry = SloRegistry::new(8, Duration::from_secs(60));
    let dimensions = SloDimensions::new().with_component("core");
    registry.record_operation(dimensions.clone(), OutcomeClass::Success, None);
    let first = registry.snapshot();
    assert_eq!(
        first.series[0].current.sample_state(),
        SloSampleState::InsufficientData
    );
    assert_eq!(first.series[0].current.success_ratio(), None);

    for _ in 1..MIN_SLO_OPERATION_SAMPLES {
        registry.record_operation(dimensions.clone(), OutcomeClass::Success, None);
    }
    let enough = registry.snapshot();
    assert_eq!(enough.series[0].current.sample_state(), SloSampleState::Sufficient);
    assert_eq!(enough.series[0].current.success_ratio(), Some(1.0));
}

#[test]
fn repeated_equivalent_health_signals_are_coalesced_with_explicit_transitions() {
    let clock = Arc::new(FakeClock::new());
    let registry = HealthRegistry::with_clock(8, clock.clone());
    registry.publish(HealthSignal::new("engine", HealthState::Healthy, "ready").unwrap());
    clock.advance(Duration::from_secs(1));
    registry.publish(HealthSignal::new("engine", HealthState::Healthy, "ready").unwrap());
    assert_eq!(registry.coalesced_count(), 1);

    registry.publish(
        HealthSignal::new("engine", HealthState::Degraded, "queue_pressure").unwrap(),
    );
    let snapshot = registry.snapshot();
    assert_eq!(snapshot.signals.len(), 1);
    assert_eq!(snapshot.signals[0].previous_state, Some(HealthState::Healthy));
    assert_eq!(snapshot.signals[0].transition_count, 1);
}

fn wait_until(mut predicate: impl FnMut() -> bool) {
    for _ in 0..200 {
        if predicate() {
            return;
        }
        thread::sleep(Duration::from_millis(5));
    }
    assert!(predicate(), "condition did not become true within bounded test wait");
}
