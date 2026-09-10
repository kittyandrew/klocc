#[macro_use]
extern crate rocket;
use prometheus::TextEncoder;

mod body;
mod cors;
mod counter;
mod data;
mod endpoints;
mod prom;
mod utils;

#[launch]
fn rocket() -> _ {
    // Since there is a hack used to lazy load those constants, we want to reset them here,
    // so they are immediately usable from start (they will be recognized by the prometheus
    // gatherer and encoder, as opposed to not shown until first increment).
    prom::TOTAL_REQUESTS_SERVED.reset();
    prom::TOTAL_REPOSITORIES_SERVED.reset();

    rocket::build()
        // Register our endpoints with /api/ root prefix.
        .mount("/api", routes![endpoints::post_klocc_job, endpoints::get_health,])
        .mount("/", routes![endpoints::get_metrics, cors::preflight])
        // Managing cache mutex. This allows rocket to pass this instance to us in any handler where we need
        // it, using rocket's internal 'State' wrapper.
        .manage(data::init_db())
        .attach(cors::CorsHeaders)
        // Encoder for the prometheus metadata.
        .manage(TextEncoder::new())
        // Adding Prometheus metadata collection middleware.
        .attach(prom::PrometheusCollection)
}
