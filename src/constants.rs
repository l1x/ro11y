/// Span field name constants.
///
/// Use these when creating spans that OtlpLayer should capture.
/// In particular, a span field named `TRACE_ID` with a 32-char hex value
/// will be parsed by OtlpLayer's FieldCollector as the W3C trace ID.
pub mod fields {
    pub const TRACE_ID: &str = "trace_id";
    pub const SPAN_ID: &str = "span_id";
    pub const HTTP_METHOD: &str = "http.method";
    pub const HTTP_URI: &str = "http.uri";
    pub const HTTP_STATUS_CODE: &str = "http.status_code";
    pub const HTTP_LATENCY_MS: &str = "http.latency_ms";
    pub const CF_REQUEST_ID: &str = "cf.request_id";
}

/// Metric event name constants.
///
/// Used in `tracing::info!` events with `metric`, `type`, and `value` fields.
/// These are converted to actual metrics downstream by Vector's `log_to_metric` transform.
pub mod metrics {
    pub const REQUEST_DURATION: &str = "http.server.request.duration";
    pub const REQUEST_COUNT: &str = "http.server.request.count";
    pub const ERROR_COUNT: &str = "http.server.error.count";
}
