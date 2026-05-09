mod noop;
pub use noop::NoopTracer;

#[cfg(feature = "tracing")]
mod otel;

#[cfg(feature = "tracing")]
pub use otel::OtelTracer;

pub trait Tracer: Send + Sync {
    fn span(&self, name: &str) -> Span;
}

pub struct Span;

impl Span {
    pub fn set_attribute(self, _key: &str, _value: &str) -> Self {
        self
    }
}

// Default: use NoopTracer when tracing feature is disabled
#[cfg(not(feature = "tracing"))]
pub type DefaultTracer = NoopTracer;

#[cfg(feature = "tracing")]
pub type DefaultTracer = OtelTracer;
