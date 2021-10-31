use prometheus::{register_int_counter, IntCounter};
use rocket::fairing::{Fairing, Info, Kind};
use rocket::{Request, Response};
use lazy_static::lazy_static;
use rocket::http::Method;


lazy_static! {
    pub static ref TOTAL_REQUESTS_SERVED: IntCounter =
        register_int_counter!("klocc_total_requests_served", "Total number of requests served at /api/jobs endpoint").unwrap();

    pub static ref TOTAL_REPOSITORIES_SERVED: IntCounter =
        register_int_counter!("klocc_total_repositories_served", "Total number of repositories processed and analyzed").unwrap();
}


pub struct PrometheusCollection;


#[rocket::async_trait]
impl Fairing for PrometheusCollection {
    fn info(&self) -> Info {
        Info {
            name: "Middleware that collects data for export to prometheus",
            kind: Kind::Response
        }
    }

    async fn on_response<'r>(&self, req: &'r Request<'_>, _res: &mut Response<'r>) {
        if req.method() == Method::Post && req.uri().path() == "/api/jobs" {
            TOTAL_REQUESTS_SERVED.inc();
        }
    }
}
