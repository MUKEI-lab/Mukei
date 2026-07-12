//! Privacy-safe correlation context for cross-layer operational diagnostics.

use std::fmt;

use sha2::{Digest, Sha256};

use crate::diagnostics::redaction::{is_safe_opaque_input, sanitize_stable_identifier};

pub const MAX_CONTEXT_ID_INPUT_LEN: usize = 256;
pub const MAX_COMPONENT_NAME_LEN: usize = 64;
pub const MAX_CONTEXT_DIMENSION_LEN: usize = 48;

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct CorrelationContext {
    request_id: Option<String>,
    operation_id: Option<String>,
    correlation_id: Option<String>,
    parent_operation_id: Option<String>,
    tenant_id: Option<String>,
    workspace_id: Option<String>,
    actor_id: Option<String>,
    component: Option<String>,
    dimensions: ContextDimensions,
}

impl CorrelationContext {
    pub fn empty() -> Self {
        Self {
            request_id: None,
            operation_id: None,
            correlation_id: None,
            parent_operation_id: None,
            tenant_id: None,
            workspace_id: None,
            actor_id: None,
            component: None,
            dimensions: ContextDimensions::default(),
        }
    }

    pub fn with_request_id(mut self, value: impl AsRef<str>) -> Self {
        self.request_id = fingerprint_opaque_id(value.as_ref());
        self
    }

    pub fn with_operation_id(mut self, value: impl AsRef<str>) -> Self {
        self.operation_id = fingerprint_opaque_id(value.as_ref());
        self
    }

    pub fn with_correlation_id(mut self, value: impl AsRef<str>) -> Self {
        self.correlation_id = fingerprint_opaque_id(value.as_ref());
        self
    }

    pub fn with_tenant_id(mut self, value: impl AsRef<str>) -> Self {
        self.tenant_id = fingerprint_opaque_id(value.as_ref());
        self
    }

    pub fn with_workspace_id(mut self, value: impl AsRef<str>) -> Self {
        self.workspace_id = fingerprint_opaque_id(value.as_ref());
        self
    }

    pub fn with_actor_id(mut self, value: impl AsRef<str>) -> Self {
        self.actor_id = fingerprint_opaque_id(value.as_ref());
        self
    }

    pub fn with_component(mut self, value: impl AsRef<str>) -> Self {
        self.component = sanitize_stable_identifier(value.as_ref(), MAX_COMPONENT_NAME_LEN);
        self
    }

    pub fn with_dimensions(mut self, dimensions: ContextDimensions) -> Self {
        self.dimensions = dimensions;
        self
    }

    /// Create nested work while preserving the root correlation identity.
    /// The caller supplies the child operation identity; no global counter is
    /// required or introduced.
    pub fn child(&self, child_operation_id: impl AsRef<str>) -> Self {
        let mut child = self.clone();
        child.parent_operation_id = self
            .operation_id
            .clone()
            .or_else(|| self.parent_operation_id.clone());
        child.operation_id = fingerprint_opaque_id(child_operation_id.as_ref());
        child
    }

    pub fn child_with_component(
        &self,
        child_operation_id: impl AsRef<str>,
        component: impl AsRef<str>,
    ) -> Self {
        self.child(child_operation_id).with_component(component)
    }

    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    pub fn operation_id(&self) -> Option<&str> {
        self.operation_id.as_deref()
    }

    pub fn correlation_id(&self) -> Option<&str> {
        self.correlation_id.as_deref()
    }

    pub fn parent_operation_id(&self) -> Option<&str> {
        self.parent_operation_id.as_deref()
    }

    pub fn tenant_id(&self) -> Option<&str> {
        self.tenant_id.as_deref()
    }

    pub fn workspace_id(&self) -> Option<&str> {
        self.workspace_id.as_deref()
    }

    pub fn actor_id(&self) -> Option<&str> {
        self.actor_id.as_deref()
    }

    pub fn component(&self) -> Option<&str> {
        self.component.as_deref()
    }

    pub fn dimensions(&self) -> &ContextDimensions {
        &self.dimensions
    }

    pub(crate) fn estimated_size_bytes(&self) -> usize {
        option_len(&self.request_id)
            .saturating_add(option_len(&self.operation_id))
            .saturating_add(option_len(&self.correlation_id))
            .saturating_add(option_len(&self.parent_operation_id))
            .saturating_add(option_len(&self.tenant_id))
            .saturating_add(option_len(&self.workspace_id))
            .saturating_add(option_len(&self.actor_id))
            .saturating_add(option_len(&self.component))
            .saturating_add(self.dimensions.estimated_size_bytes())
    }
}

impl Default for CorrelationContext {
    fn default() -> Self {
        Self::empty()
    }
}

impl fmt::Debug for CorrelationContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CorrelationContext")
            .field("request_id", &presence(self.request_id.as_ref()))
            .field("operation_id", &presence(self.operation_id.as_ref()))
            .field("correlation_id", &presence(self.correlation_id.as_ref()))
            .field(
                "parent_operation_id",
                &presence(self.parent_operation_id.as_ref()),
            )
            .field("tenant_id", &presence(self.tenant_id.as_ref()))
            .field("workspace_id", &presence(self.workspace_id.as_ref()))
            .field("actor_id", &presence(self.actor_id.as_ref()))
            .field("component", &self.component)
            .field("dimensions", &self.dimensions)
            .finish()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ContextDimensions {
    operation_kind: Option<String>,
    model_family: Option<String>,
    backend_kind: Option<String>,
    feature_area: Option<String>,
    result_class: Option<String>,
}

impl ContextDimensions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_operation_kind(mut self, value: impl AsRef<str>) -> Self {
        self.operation_kind = stable_dimension(value.as_ref());
        self
    }

    pub fn with_model_family(mut self, value: impl AsRef<str>) -> Self {
        self.model_family = stable_dimension(value.as_ref());
        self
    }

    pub fn with_backend_kind(mut self, value: impl AsRef<str>) -> Self {
        self.backend_kind = stable_dimension(value.as_ref());
        self
    }

    pub fn with_feature_area(mut self, value: impl AsRef<str>) -> Self {
        self.feature_area = stable_dimension(value.as_ref());
        self
    }

    pub fn with_result_class(mut self, value: impl AsRef<str>) -> Self {
        self.result_class = stable_dimension(value.as_ref());
        self
    }

    pub fn operation_kind(&self) -> Option<&str> {
        self.operation_kind.as_deref()
    }

    pub fn model_family(&self) -> Option<&str> {
        self.model_family.as_deref()
    }

    pub fn backend_kind(&self) -> Option<&str> {
        self.backend_kind.as_deref()
    }

    pub fn feature_area(&self) -> Option<&str> {
        self.feature_area.as_deref()
    }

    pub fn result_class(&self) -> Option<&str> {
        self.result_class.as_deref()
    }

    pub(crate) fn estimated_size_bytes(&self) -> usize {
        option_len(&self.operation_kind)
            .saturating_add(option_len(&self.model_family))
            .saturating_add(option_len(&self.backend_kind))
            .saturating_add(option_len(&self.feature_area))
            .saturating_add(option_len(&self.result_class))
    }
}

fn stable_dimension(value: &str) -> Option<String> {
    sanitize_stable_identifier(value, MAX_CONTEXT_DIMENSION_LEN)
}

fn fingerprint_opaque_id(value: &str) -> Option<String> {
    if !is_safe_opaque_input(value, MAX_CONTEXT_ID_INPUT_LEN) {
        return None;
    }

    let mut hasher = Sha256::new();
    hasher.update(b"mukei-diagnostics-context-v1\0");
    hasher.update(value.trim().as_bytes());
    let digest = hasher.finalize();
    let mut encoded = String::with_capacity(28);
    encoded.push_str("ctx_");
    for byte in digest.iter().take(12) {
        use std::fmt::Write as _;
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    Some(encoded)
}

fn presence(value: Option<&String>) -> &'static str {
    if value.is_some() {
        "<opaque-present>"
    } else {
        "<none>"
    }
}

fn option_len(value: &Option<String>) -> usize {
    value.as_ref().map_or(0, String::len)
}
