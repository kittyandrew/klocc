#[macro_use] extern crate rocket;


mod data;
mod body;
mod cors;
mod utils;
mod counter;
mod endpoints;


#[launch]
fn rocket() -> _ {
    rocket::build()
        .mount("/api", routes![endpoints::post_klocc_job])
        // TODO: All-catchers
        //.register(catchers![
        //    misc::not_found,
        //    misc::unauth_handler,
        //    misc::serverside_handler,
        //])
        // Managing mutex-es.
        .manage(data::init_db())
        // Adding CORS headers fairing.
        .attach(cors::CORS)
}

