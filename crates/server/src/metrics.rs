use prometheus::{
    register_gauge_vec_with_registry as gauge, register_histogram_vec_with_registry as histogram,
    register_int_counter_vec_with_registry as counter, Encoder, GaugeVec, HistogramVec, IntCounterVec, Registry,
    TextEncoder,
};

/// The `board_source_*` metrics, labelled by source, and the HTTP ones.
pub struct Metrics {
    registry: Registry,
    reads: IntCounterVec,
    errors: IntCounterVec,
    duration: HistogramVec,
    age: GaugeVec,
    requests: IntCounterVec,
    request_duration: HistogramVec,
}

impl Default for Metrics {
    fn default() -> Self {
        let r = Registry::new();
        let s = &["source"];
        let buckets = vec![0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 20.0, 40.0];
        Self {
            reads: counter!("board_source_reads_total", "Reads of each source.", s, r).unwrap(),
            errors: counter!("board_source_read_errors_total", "Reads of each source that failed.", s, r).unwrap(),
            duration: histogram!("board_source_read_duration_seconds", "Read time, retry included.", s, buckets, r)
                .unwrap(),
            age: gauge!("board_source_age_seconds", "Seconds since the source's last good read.", s, r).unwrap(),
            requests: counter!("board_http_requests_total", "HTTP requests.", &["route", "status"], r).unwrap(),
            request_duration: histogram!("board_http_request_duration_seconds", "HTTP time.", &["route"], r).unwrap(),
            registry: r,
        }
    }
}

impl Metrics {
    pub fn observe(&self, source: &str, secs: f64, ok: bool) {
        self.reads.with_label_values(&[source]).inc();
        // Touch the error counter so it's exported at 0 before the first failure.
        let errors = self.errors.with_label_values(&[source]);
        if !ok {
            errors.inc();
        }
        self.duration.with_label_values(&[source]).observe(secs);
    }

    pub fn request(&self, route: &str, status: u16, secs: f64) {
        self.requests.with_label_values(&[route, &status.to_string()]).inc();
        self.request_duration.with_label_values(&[route]).observe(secs);
    }

    pub fn render(&self, ages: &[(&str, f64)]) -> String {
        for (source, age) in ages {
            self.age.with_label_values(&[source]).set(*age);
        }
        let mut buf = Vec::new();
        TextEncoder::new().encode(&self.registry.gather(), &mut buf).unwrap_or_default();
        String::from_utf8(buf).unwrap_or_default()
    }
}
