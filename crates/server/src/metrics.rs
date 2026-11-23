use prometheus::{Encoder, GaugeVec, HistogramOpts, HistogramVec, IntCounterVec, Opts, Registry, TextEncoder};

/// The `board_source_*` metrics, labelled by source.
pub struct Metrics {
    registry: Registry,
    reads: IntCounterVec,
    errors: IntCounterVec,
    duration: HistogramVec,
    age: GaugeVec,
}

impl Default for Metrics {
    fn default() -> Self {
        let l = &["source"];
        let reads = IntCounterVec::new(Opts::new("board_source_reads_total", "Reads of each source."), l).unwrap();
        let errors =
            IntCounterVec::new(Opts::new("board_source_read_errors_total", "Reads of each source that failed."), l)
                .unwrap();
        let duration = HistogramVec::new(
            HistogramOpts::new("board_source_read_duration_seconds", "How long each source read took, retry included.")
                .buckets(vec![0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 20.0, 40.0]),
            l,
        )
        .unwrap();
        let age = GaugeVec::new(Opts::new("board_source_age_seconds", "Seconds since the source's last good read."), l)
            .unwrap();
        let registry = Registry::new();
        for c in [
            Box::new(reads.clone()) as Box<dyn prometheus::core::Collector>,
            Box::new(errors.clone()),
            Box::new(duration.clone()),
            Box::new(age.clone()),
        ] {
            registry.register(c).unwrap();
        }
        Self { registry, reads, errors, duration, age }
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

    pub fn render(&self, ages: &[(&str, f64)]) -> String {
        for (source, age) in ages {
            self.age.with_label_values(&[source]).set(*age);
        }
        let mut buf = Vec::new();
        TextEncoder::new().encode(&self.registry.gather(), &mut buf).unwrap_or_default();
        String::from_utf8(buf).unwrap_or_default()
    }
}
