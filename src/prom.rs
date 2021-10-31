use prometheus::{register_int_counter, register_histogram_vec, IntCounter, HistogramVec};
use rocket::fairing::{Fairing, Info, Kind};
use rocket::{Request, Response, Data};
use lazy_static::lazy_static;
use rocket::http::Method;
use std::time::Instant;


lazy_static! {
    pub static ref TOTAL_REQUESTS_SERVED: IntCounter =
        register_int_counter!("klocc_total_requests_served", "Total number of requests served at /api/jobs endpoint").unwrap();

    pub static ref TOTAL_REPOSITORIES_SERVED: IntCounter =
        register_int_counter!("klocc_total_repositories_served", "Total number of repositories processed and analyzed").unwrap();

    pub static ref JOB_REQUESTS_DURATION: HistogramVec = register_histogram_vec!(
        "klocc_jobs_requests_duration_seconds", "Jobs endpoint latencies in seconds", &["handler"]
    ).unwrap();

    pub static ref JOB_RESPONSE_SIZE_BYTES: HistogramVec = register_histogram_vec!(
        "klocc_jobs_response_size_bytes", "Jobs endpoint response body size in bytes", &["handler"],
        vec![256., 1024., 4096., 16384., 65536., 262144., 1048576., 4194304.],
    ).unwrap();
}


pub struct PrometheusCollection;


#[derive(Copy, Clone, Debug)]
struct DurationTimer(Option<Instant>);


#[rocket::async_trait]
impl Fairing for PrometheusCollection {
    fn info(&self) -> Info {
        Info {
            name: "Prometheus metrics",
            kind: Kind::Request | Kind::Response
        }
    }

    async fn on_request(&self, request: &mut Request<'_>, _: &mut Data<'_>) {
        // Note(andrew): Setting timer value here to the local request cache, since this
        //     is executed prior to entering our endpoints, which means we will be able
        //     to time request latency.
        request.local_cache(|| DurationTimer(Some(Instant::now())));
    }

    async fn on_response<'r>(&self, request: &'r Request<'_>, response: &mut Response<'r>) {
        if request.method() == Method::Post && request.uri().path() == "/api/jobs" {
            // Measuring request latency (internal processing time). Doing this first, so any
            // code below adds minimal overhead to the processing time. Just make sure that the
            // code below actually isn't artificially slow.
            let duration_timer = request.local_cache(|| DurationTimer(None));
            if let Some(duration) = duration_timer.0.map(|st| st.elapsed()) {
                let latency_ms = duration.as_millis();

                JOB_REQUESTS_DURATION.local().with_label_values(&["all"]).observe(latency_ms as f64 / 1000.);
                // While we can, lets add response header with timing as well.
                response.set_raw_header("X-Response-Time", format!("{} ms", latency_ms));
            };

            // Record total request count.
            TOTAL_REQUESTS_SERVED.inc();

            // Record response body size.
            if let Some(body_size) = response.body_mut().size().await {
                JOB_RESPONSE_SIZE_BYTES.local().with_label_values(&["all"]).observe(body_size as f64);
            };
        }
    }
}
