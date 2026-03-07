use crate::metrics::MetricSnapshot;
use crate::otlp_trace::{encode_resource, encode_scope, KeyValue};
use crate::proto::*;

/// Encode a KeyValue from (String, String) attribute pairs.
/// Uses field 1 = key (string), field 2 = value (AnyValue message with string_value field 1).
fn encode_attr_key_value(buf: &mut Vec<u8>, key: &str, value: &str) {
    encode_string_field(buf, 1, key);
    encode_message_field_in_place(buf, 2, |buf| {
        encode_string_field(buf, 1, value); // AnyValue.string_value = field 1
    });
}

/// Encode a NumberDataPoint for a counter (as_int, field 6 = sfixed64).
///
/// NumberDataPoint fields:
///   attributes(7), start_time_unix_nano(2), time_unix_nano(3), as_int(6)
fn encode_counter_data_point(
    buf: &mut Vec<u8>,
    attrs: &[(String, String)],
    start_time_unix_nano: u64,
    time_unix_nano: u64,
    value: i64,
) {
    encode_fixed64_field(buf, 2, start_time_unix_nano);
    encode_fixed64_field(buf, 3, time_unix_nano);
    // as_int: field 6, fixed64 (sfixed64 on the wire)
    encode_fixed64_field_always(buf, 6, value as u64);
    for (k, v) in attrs {
        encode_message_field_in_place(buf, 7, |buf| {
            encode_attr_key_value(buf, k, v);
        });
    }
}

/// Encode a NumberDataPoint for a gauge (as_double, field 4 = fixed64).
///
/// NumberDataPoint fields:
///   attributes(7), start_time_unix_nano(2), time_unix_nano(3), as_double(4)
fn encode_gauge_data_point(
    buf: &mut Vec<u8>,
    attrs: &[(String, String)],
    start_time_unix_nano: u64,
    time_unix_nano: u64,
    value: f64,
) {
    encode_fixed64_field(buf, 2, start_time_unix_nano);
    encode_fixed64_field(buf, 3, time_unix_nano);
    // as_double: field 4, fixed64
    encode_fixed64_field_always(buf, 4, value.to_bits());
    for (k, v) in attrs {
        encode_message_field_in_place(buf, 7, |buf| {
            encode_attr_key_value(buf, k, v);
        });
    }
}

/// Encode a Sum message (field 7 of Metric).
///
/// Sum fields:
///   data_points(1), aggregation_temporality(2), is_monotonic(3)
fn encode_sum(
    buf: &mut Vec<u8>,
    data_points: &[(Vec<(String, String)>, i64)],
    start_time: u64,
    time: u64,
) {
    for (attrs, value) in data_points {
        encode_message_field_in_place(buf, 1, |buf| {
            encode_counter_data_point(buf, attrs, start_time, time, *value);
        });
    }
    // CUMULATIVE = 2
    encode_varint_field(buf, 2, 2);
    // is_monotonic = true
    encode_varint_field(buf, 3, 1);
}

/// Encode a Gauge message (field 5 of Metric).
///
/// Gauge fields:
///   data_points(1)
fn encode_gauge_msg(
    buf: &mut Vec<u8>,
    data_points: &[(Vec<(String, String)>, f64)],
    start_time: u64,
    time: u64,
) {
    for (attrs, value) in data_points {
        encode_message_field_in_place(buf, 1, |buf| {
            encode_gauge_data_point(buf, attrs, start_time, time, *value);
        });
    }
}

/// Encode a single Metric message.
///
/// Metric fields:
///   name(1), description(2), unit(3), gauge(5), sum(7)
fn encode_metric(buf: &mut Vec<u8>, snapshot: &MetricSnapshot, start_time: u64, time: u64) {
    match snapshot {
        MetricSnapshot::Counter {
            name,
            description,
            data_points,
        } => {
            encode_string_field(buf, 1, name);
            encode_string_field(buf, 2, description);
            // unit (field 3) — empty, skip
            // Sum (field 7)
            encode_message_field_in_place(buf, 7, |buf| {
                encode_sum(buf, data_points, start_time, time);
            });
        }
        MetricSnapshot::Gauge {
            name,
            description,
            data_points,
        } => {
            encode_string_field(buf, 1, name);
            encode_string_field(buf, 2, description);
            // Gauge (field 5)
            encode_message_field_in_place(buf, 5, |buf| {
                encode_gauge_msg(buf, data_points, start_time, time);
            });
        }
    }
}

/// Encode a full ExportMetricsServiceRequest.
///
/// Structure:
///   ExportMetricsServiceRequest { resource_metrics: [ResourceMetrics] }
///     ResourceMetrics { resource(1), scope_metrics(2) }
///       ScopeMetrics { scope(1), metrics(2) }
pub fn encode_export_metrics_request(
    resource_attrs: &[KeyValue],
    scope_name: &str,
    scope_version: &str,
    snapshots: &[MetricSnapshot],
    start_time_unix_nano: u64,
    time_unix_nano: u64,
) -> Vec<u8> {
    let mut request_buf = Vec::new();
    // ResourceMetrics (field 1 of ExportMetricsServiceRequest)
    encode_message_field_in_place(&mut request_buf, 1, |buf| {
        // Resource (field 1 of ResourceMetrics)
        encode_message_field_in_place(buf, 1, |buf| {
            encode_resource(buf, resource_attrs);
        });
        // ScopeMetrics (field 2 of ResourceMetrics)
        encode_message_field_in_place(buf, 2, |buf| {
            // InstrumentationScope (field 1 of ScopeMetrics)
            encode_message_field_in_place(buf, 1, |buf| {
                encode_scope(buf, scope_name, scope_version);
            });
            // Metrics (field 2 of ScopeMetrics, repeated)
            for snapshot in snapshots {
                encode_message_field_in_place(buf, 2, |buf| {
                    encode_metric(buf, snapshot, start_time_unix_nano, time_unix_nano);
                });
            }
        });
    });
    request_buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_counter_metric_is_nonempty() {
        let snapshots = vec![MetricSnapshot::Counter {
            name: "http_requests_total".to_string(),
            description: "Total HTTP requests".to_string(),
            data_points: vec![(
                vec![
                    ("method".to_string(), "GET".to_string()),
                    ("status".to_string(), "200".to_string()),
                ],
                42,
            )],
        }];

        let bytes = encode_export_metrics_request(
            &[KeyValue {
                key: "service.name".to_string(),
                value: crate::otlp_trace::AnyValue::String("test-svc".to_string()),
            }],
            "ro11y",
            "0.3.0",
            &snapshots,
            1_000_000_000,
            2_000_000_000,
        );

        assert!(!bytes.is_empty());
        assert_eq!(bytes[0], 0x0A); // field 1, wire type 2

        // Verify the metric name is in the output
        let name = b"http_requests_total";
        assert!(
            bytes.windows(name.len()).any(|w| w == name),
            "metric name not found in encoded bytes"
        );
    }

    #[test]
    fn encode_gauge_metric_is_nonempty() {
        let snapshots = vec![MetricSnapshot::Gauge {
            name: "cpu_usage".to_string(),
            description: "CPU usage percentage".to_string(),
            data_points: vec![(vec![("core".to_string(), "0".to_string())], 75.5)],
        }];

        let bytes = encode_export_metrics_request(
            &[KeyValue {
                key: "service.name".to_string(),
                value: crate::otlp_trace::AnyValue::String("test-svc".to_string()),
            }],
            "ro11y",
            "0.3.0",
            &snapshots,
            1_000_000_000,
            2_000_000_000,
        );

        assert!(!bytes.is_empty());
        // Verify gauge value is encoded (75.5 as f64 bits, little-endian)
        let val_bytes = 75.5_f64.to_bits().to_le_bytes();
        assert!(
            bytes.windows(8).any(|w| w == val_bytes),
            "gauge value not found in encoded bytes"
        );
    }

    #[test]
    fn encode_counter_value_is_correct() {
        let snapshots = vec![MetricSnapshot::Counter {
            name: "c".to_string(),
            description: String::new(),
            data_points: vec![(vec![], 99)],
        }];

        let bytes = encode_export_metrics_request(&[], "ro11y", "0.3.0", &snapshots, 0, 0);

        // as_int value 99 encoded as fixed64 LE
        let val_bytes = (99_i64 as u64).to_le_bytes();
        assert!(
            bytes.windows(8).any(|w| w == val_bytes),
            "counter value 99 not found in encoded bytes"
        );
    }

    #[test]
    fn encode_multiple_data_points() {
        let snapshots = vec![MetricSnapshot::Counter {
            name: "multi".to_string(),
            description: String::new(),
            data_points: vec![
                (vec![("k".to_string(), "a".to_string())], 10),
                (vec![("k".to_string(), "b".to_string())], 20),
            ],
        }];

        let bytes = encode_export_metrics_request(&[], "ro11y", "0.3.0", &snapshots, 0, 0);

        // Both values should be present
        let val10 = (10_i64 as u64).to_le_bytes();
        let val20 = (20_i64 as u64).to_le_bytes();
        assert!(bytes.windows(8).any(|w| w == val10));
        assert!(bytes.windows(8).any(|w| w == val20));
    }

    #[test]
    fn encode_counter_has_cumulative_temporality() {
        let snapshots = vec![MetricSnapshot::Counter {
            name: "c".to_string(),
            description: String::new(),
            data_points: vec![(vec![], 1)],
        }];

        let bytes = encode_export_metrics_request(&[], "ro11y", "0.3.0", &snapshots, 0, 0);

        // aggregation_temporality = 2 (CUMULATIVE): varint field 2, value 2
        // tag = (2<<3)|0 = 0x10, value = 0x02
        assert!(
            bytes.windows(2).any(|w| w == [0x10, 0x02]),
            "CUMULATIVE temporality not found"
        );
    }

    #[test]
    fn encode_counter_is_monotonic() {
        let snapshots = vec![MetricSnapshot::Counter {
            name: "c".to_string(),
            description: String::new(),
            data_points: vec![(vec![], 1)],
        }];

        let bytes = encode_export_metrics_request(&[], "ro11y", "0.3.0", &snapshots, 0, 0);

        // is_monotonic = true: varint field 3, value 1
        // tag = (3<<3)|0 = 0x18, value = 0x01
        assert!(
            bytes.windows(2).any(|w| w == [0x18, 0x01]),
            "is_monotonic=true not found"
        );
    }

    #[test]
    fn encode_mixed_counter_and_gauge() {
        let snapshots = vec![
            MetricSnapshot::Counter {
                name: "requests".to_string(),
                description: String::new(),
                data_points: vec![(vec![], 100)],
            },
            MetricSnapshot::Gauge {
                name: "temperature".to_string(),
                description: String::new(),
                data_points: vec![(vec![], 36.6)],
            },
        ];

        let bytes = encode_export_metrics_request(&[], "ro11y", "0.3.0", &snapshots, 0, 0);

        assert!(bytes.windows(8).any(|w| w == b"requests"));
        assert!(bytes.windows(11).any(|w| w == b"temperature"));
    }

    #[test]
    fn encode_attributes_in_data_point() {
        let snapshots = vec![MetricSnapshot::Counter {
            name: "c".to_string(),
            description: String::new(),
            data_points: vec![(
                vec![("method".to_string(), "GET".to_string())],
                1,
            )],
        }];

        let bytes = encode_export_metrics_request(&[], "ro11y", "0.3.0", &snapshots, 0, 0);

        assert!(bytes.windows(6).any(|w| w == b"method"));
        assert!(bytes.windows(3).any(|w| w == b"GET"));
    }
}
