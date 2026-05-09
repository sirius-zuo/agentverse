use super::{Span, Tracer};

pub struct NoopTracer;

impl Tracer for NoopTracer {
    fn span(&self, _name: &str) -> Span {
        Span
    }
}
