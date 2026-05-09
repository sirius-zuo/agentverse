use async_trait::async_trait;
use std::sync::Arc;

pub struct OtelTracer {
    _inner: Arc<()>,
}

impl OtelTracer {
    pub fn new() -> Self {
        Self {
            _inner: Arc::new(()),
        }
    }
}

#[async_trait]
impl crate::tracing::Tracer for OtelTracer {
    fn span(&self, _name: &str) -> crate::tracing::Span {
        crate::tracing::Span
    }
}

impl Default for OtelTracer {
    fn default() -> Self {
        Self::new()
    }
}
