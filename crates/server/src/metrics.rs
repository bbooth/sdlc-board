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
    app_tokens: IntCounterVec,
}

impl Default for Metrics {
    fn default() -> Self {
        let r = Registry::new();
        let s = &["source"];
        let b = vec![0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 20.0, 40.0];
        Self {
            reads: counter!("board_source_reads_total", "Reads of each source.", s, r).unwrap(),
            errors: counter!("board_source_read_errors_total", "Reads that failed, in whole or in part.", s, r)
                .unwrap(),
            duration: histogram!("board_source_read_duration_seconds", "Read time, retry included.", s, b, r).unwrap(),
            age: gauge!("board_source_age_seconds", "Seconds since the source's last good read.", s, r).unwrap(),
            requests: counter!("board_http_requests_total", "HTTP requests.", &["route", "status"], r).unwrap(),
            request_duration: histogram!("board_http_request_duration_seconds", "HTTP time.", &["route"], r).unwrap(),
            app_tokens: counter!(
                "board_github_app_tokens_total",
                "GitHub App installation tokens got.",
                &["result"],
                r
            )
            .unwrap(),
            registry: r,
        }
    }
}

impl Metrics {
    pub fn observe(&self, source: &str, secs: f64, ok: bool) {
        self.reads.with_label_values(&[source]).inc();
        // Touch the error counter so it's exported at 0 before the first failure.
        self.errors.with_label_values(&[source]).inc_by(u64::from(!ok));
        self.duration.with_label_values(&[source]).observe(secs);
    }

    pub fn app_token(&self, ok: bool) {
        self.app_tokens.with_label_values(&[if ok { "ok" } else { "error" }]).inc();
    }

    /// Exports both results at 0, so a failure shows as a change, not a new series.
    pub fn touch_app_tokens(&self) {
        for result in ["ok", "error"] {
            self.app_tokens.with_label_values(&[result]);
        }
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
