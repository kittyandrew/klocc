use rocket::serde::{json::from_str, Deserialize, Serialize};
use rocket::data::{Outcome, FromData, ByteUnit};
use rocket::http::{Status, ContentType};
use rocket::{Request, Data};


// Note(andrew): Use this constant as a hard limit for the buffer that reads request
//     body into memory, since this is more than enough for given arguments, and all
//     bigger payloads are probably an attempt to pass malicious data or to perform
//     a DoS attack.
const LIMIT: ByteUnit = ByteUnit::Byte(4096);


// Use this struct as a typed input for the POST /jobs endpoint.
#[derive(Serialize, Deserialize, Debug)]
#[serde(crate = "rocket::serde")]
pub struct PostJobData {
    pub username: String,
    pub reponame: String,
    pub provider: String,
}


#[rocket::async_trait]
impl<'r> FromData<'r> for PostJobData {
    type Error = String;

    // This function is called for parsing a body of the request, when we are expecting 'PostJobData'
    // struct as a json input for our endpoint. If this function returns 'Outcome::Error', the request
    // fails (or falls through via 'Outcome::Forward'). When this function returns 'Outcome::Success', it
    // means the struct was parsed successfully (e.g. all fields are present) and Rocket framework executes
    // the endpoint code (if all other similar dependencies are resolved successfully as well).
    async fn from_data(req: &'r Request<'_>, data: Data<'r>) -> Outcome<'r, Self> {
        // Ensure the content type is correct before reading the body.
        let json_ct = ContentType::new("application", "json");
        // If request does not contain a valid JSON header, forward to the next handler.
        if req.content_type() != Some(&json_ct) {
            return Outcome::Forward((data, Status::TemporaryRedirect));  // Early return from the handler.
        }

        // Reading body up to the 'LIMIT' amount of bytes, and try to read it as utf-8 string.
        match data.open(LIMIT).into_string().await {
            // Parsing 'PostJobData' struct from the string (as json-like).
            Ok(string) => match from_str::<PostJobData>(&string) {
                Ok(job) => return Outcome::Success(job),  // Return successfully.
                Err(e)  => return Outcome::Error((Status::BadRequest, format!("Failed to parse json: {}.", e))),
            },
            Err(e) => return Outcome::Error((Status::BadRequest, format!("Failed to read body: {}.", e)))
        };  // All codepaths return.
    }
}

