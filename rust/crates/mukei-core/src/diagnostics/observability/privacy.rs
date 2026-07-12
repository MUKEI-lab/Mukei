//! Local telemetry privacy policy, export gating and privacy epochs.

use parking_lot::RwLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TelemetryPrivacyMode {
    Disabled,
    Essential,
    Extended,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FieldSensitivity {
    PublicSafe,
    OperationalSafe,
    Sensitive,
    Secret,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EventScope {
    Essential,
    Extended,
}

/// Privacy policy for local retention and external sink export.
///
/// `Essential` and `Extended` control which structured observations may be
/// retained locally. Export is a separate, explicit opt-in. This prevents an
/// installed sink from silently turning local diagnostics into telemetry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TelemetryPolicy {
    mode: TelemetryPrivacyMode,
    export_allowed: bool,
}

impl TelemetryPolicy {
    pub const fn new(mode: TelemetryPrivacyMode) -> Self {
        Self {
            mode,
            export_allowed: false,
        }
    }

    pub const fn disabled() -> Self {
        Self::new(TelemetryPrivacyMode::Disabled)
    }

    pub const fn essential() -> Self {
        Self::new(TelemetryPrivacyMode::Essential)
    }

    pub const fn extended() -> Self {
        Self::new(TelemetryPrivacyMode::Extended)
    }

    pub const fn essential_with_export() -> Self {
        Self {
            mode: TelemetryPrivacyMode::Essential,
            export_allowed: true,
        }
    }

    pub const fn extended_with_export() -> Self {
        Self {
            mode: TelemetryPrivacyMode::Extended,
            export_allowed: true,
        }
    }

    pub const fn with_export_allowed(mut self, allowed: bool) -> Self {
        self.export_allowed = allowed && !matches!(self.mode, TelemetryPrivacyMode::Disabled);
        self
    }

    pub const fn mode(self) -> TelemetryPrivacyMode {
        self.mode
    }

    pub const fn export_allowed(self) -> bool {
        self.export_allowed && !matches!(self.mode, TelemetryPrivacyMode::Disabled)
    }

    pub const fn allows_event(self, scope: EventScope) -> bool {
        match (self.mode, scope) {
            (TelemetryPrivacyMode::Disabled, _) => false,
            (TelemetryPrivacyMode::Essential, EventScope::Essential) => true,
            (TelemetryPrivacyMode::Essential, EventScope::Extended) => false,
            (TelemetryPrivacyMode::Extended, _) => true,
        }
    }

    pub const fn allows_export(self, scope: EventScope) -> bool {
        self.export_allowed() && self.allows_event(scope)
    }

    /// Sensitive and secret fields are never admitted into structured
    /// telemetry. Extended mode broadens event coverage, not sensitivity.
    pub const fn allows_field(self, sensitivity: FieldSensitivity) -> bool {
        match self.mode {
            TelemetryPrivacyMode::Disabled => false,
            TelemetryPrivacyMode::Essential | TelemetryPrivacyMode::Extended => {
                matches!(
                    sensitivity,
                    FieldSensitivity::PublicSafe | FieldSensitivity::OperationalSafe
                )
            }
        }
    }
}

impl Default for TelemetryPolicy {
    fn default() -> Self {
        // Local essential diagnostics are available by default, but no sink
        // export is permitted until the caller explicitly opts in.
        Self::essential()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PrivacySnapshot {
    pub(crate) policy: TelemetryPolicy,
    pub(crate) epoch: u64,
}

/// Atomically pairs the active policy with its generation. Sink workers use
/// this at emission time so queued data from an older consent state is never
/// emitted after an opt-out or scope reduction.
pub(crate) struct PrivacyState {
    state: RwLock<PrivacySnapshot>,
}

impl PrivacyState {
    pub(crate) fn new(policy: TelemetryPolicy) -> Self {
        Self {
            state: RwLock::new(PrivacySnapshot { policy, epoch: 0 }),
        }
    }

    pub(crate) fn snapshot(&self) -> PrivacySnapshot {
        *self.state.read()
    }

    pub(crate) fn update(&self, policy: TelemetryPolicy) -> (PrivacySnapshot, PrivacySnapshot) {
        let mut state = self.state.write();
        let previous = *state;
        if state.policy != policy {
            state.epoch = state.epoch.saturating_add(1);
            state.policy = policy;
        }
        (previous, *state)
    }

    pub(crate) fn permits_export(&self, epoch: u64, scope: EventScope) -> bool {
        let state = self.state.read();
        state.epoch == epoch && state.policy.allows_export(scope)
    }
}
