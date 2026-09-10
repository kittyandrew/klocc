use rocket::fairing::{Fairing, Info, Kind};
use rocket::http::{Header, Method, Status};
use rocket::request::{FromRequest, Outcome};
use rocket::{Request, Response};
use url::{Origin, Url};

pub struct Cors(bool);
pub struct CorsHeaders;

fn policy<'a>(request: &'a Request<'_>) -> &'a Result<Option<String>, Status> {
    request.local_cache(|| {
        let Some(origin) = request.headers().get_one("Origin") else {
            return Ok(None);
        };
        let origin = if origin.eq_ignore_ascii_case("null") {
            "null".to_owned()
        } else {
            match Url::parse(origin).map_err(|_| Status::BadRequest)?.origin() {
                Origin::Opaque(_) => origin.to_owned(),
                origin => origin.ascii_serialization(),
            }
        };
        if request.method() == Method::Options {
            let method = request
                .headers()
                .get_one("Access-Control-Request-Method")
                .and_then(|value| value.parse::<Method>().ok())
                .ok_or(Status::BadRequest)?;
            if !matches!(method, Method::Get | Method::Post) {
                return Err(Status::Forbidden);
            }
        }
        Ok(Some(origin))
    })
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for Cors {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match policy(request) {
            Ok(origin) => Outcome::Success(Self(origin.is_some())),
            Err(status) => Outcome::Error((*status, ())),
        }
    }
}

#[rocket::async_trait]
impl Fairing for CorsHeaders {
    fn info(&self) -> Info {
        Info {
            name: "Credentialed cross-origin requests",
            kind: Kind::Response,
        }
    }

    async fn on_response<'r>(&self, request: &'r Request<'_>, response: &mut Response<'r>) {
        let Ok(Some(origin)) = policy(request) else { return };
        // @NOTE: Credentials require a reflected origin, not '*'. - Sep 10, 2026
        response.set_header(Header::new("Access-Control-Allow-Origin", origin.clone()));
        response.set_header(Header::new("Access-Control-Allow-Credentials", "true"));
        response.adjoin_header(Header::new("Vary", "Origin"));
        if request.method() == Method::Options {
            response.set_header(Header::new("Access-Control-Allow-Methods", "GET, POST"));
            response.adjoin_header(Header::new(
                "Vary",
                "Access-Control-Request-Method, Access-Control-Request-Headers",
            ));
            if let Some(headers) = request.headers().get_one("Access-Control-Request-Headers") {
                response.set_header(Header::new("Access-Control-Allow-Headers", headers.to_owned()));
            }
        }
    }
}

#[options("/<_..>")]
pub fn preflight(cors: Cors) -> Status {
    if cors.0 { Status::NoContent } else { Status::NotFound }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rocket::local::blocking::Client;

    #[test]
    fn browser_requests_preserve_the_public_cors_policy() {
        let client = Client::tracked(crate::rocket()).unwrap();
        for origin in ["https://client.example", "null"] {
            for path in ["/api/health", "/metrics"] {
                let response = client.get(path).header(Header::new("Origin", origin)).dispatch();
                assert_eq!(response.status(), Status::Ok);
                assert_eq!(response.headers().get_one("Access-Control-Allow-Origin"), Some(origin));
                assert_eq!(
                    response.headers().get_one("Access-Control-Allow-Credentials"),
                    Some("true")
                );
                assert!(response.headers().get("Vary").any(|value| value.contains("Origin")));
            }
        }
        for path in ["/api/jobs", "/unknown"] {
            let response = client
                .options(path)
                .header(Header::new("Origin", "https://client.example"))
                .header(Header::new("Access-Control-Request-Method", "POST"))
                .header(Header::new(
                    "Access-Control-Request-Headers",
                    "authorization, content-type, x-custom",
                ))
                .dispatch();
            assert_eq!(response.status(), Status::NoContent);
            assert_eq!(
                response.headers().get_one("Access-Control-Allow-Methods"),
                Some("GET, POST")
            );
            assert_eq!(
                response.headers().get_one("Access-Control-Allow-Headers"),
                Some("authorization, content-type, x-custom")
            );
        }
        assert_eq!(client.options("/api/jobs").dispatch().status(), Status::NotFound);
        assert!(
            client
                .get("/api/health")
                .dispatch()
                .headers()
                .get_one("Access-Control-Allow-Origin")
                .is_none()
        );
        for (method, status) in [("DELETE", Status::Forbidden), ("INVALID", Status::BadRequest)] {
            let response = client
                .options("/api/jobs")
                .header(Header::new("Origin", "https://client.example"))
                .header(Header::new("Access-Control-Request-Method", method))
                .dispatch();
            assert_eq!(response.status(), status);
            assert!(response.headers().get_one("Access-Control-Allow-Origin").is_none());
        }
        let response = client
            .options("/api/jobs")
            .header(Header::new("Origin", "https://client.example"))
            .dispatch();
        assert_eq!(response.status(), Status::BadRequest);
        let response = client
            .post("/api/jobs")
            .header(rocket::http::ContentType::JSON)
            .header(Header::new("Origin", "invalid"))
            .dispatch();
        assert_eq!(response.status(), Status::BadRequest);
        assert!(response.headers().get_one("Access-Control-Allow-Origin").is_none());
    }
}
