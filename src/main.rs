#[macro_use] extern crate rocket;
use rocket_cors::{AllowedHeaders, AllowedOrigins};
use rocket::http::Method;


mod data;
mod body;
mod utils;
mod counter;
mod endpoints;


#[launch]
fn rocket() -> _ {
    // Note(andrew): Here we use additional rocket crate to handle browser configurations for us, because
    //     without this, browsers refuse to call into our API, because they lack headers (not that hard to
    //     add), and also lack handlers for OPTIONS with proper response, which is one of the main reasons
    //     I decided to use the crate - less polution in our code. In addition to that, I also don't really
    //     know much about implementing that, and whether those are complete requirements, so the prospect
    //     of me implementing this myself is not very fun, and it's definitely not going to be very robust.
    //     Although of course, using remote code (i.e. crate) is arguably worse than adding anything locally
    //     (source code vs dependencies), but, honestly, I don't want to deal with browser bullshit a single
    //     bit right now. Maybe my mood will change later, or someone who knows a lot about browser stuff can
    //     contribute to this project by writing a robust clean handling of the CORS for our service.  @Robustness @HelpNeeded
    let cors = rocket_cors::CorsOptions {
        // Note(andrew): Allowing all origins to call into this. This is a sneaky browser thing, where you
        //     can configure 'Access-Control-Allow-Origin' header on the backend like this one, to forbid
        //     any website, except yours, to call into the API from their javascript (frontend code). This
        //     current config allows anyone to do the calling, but you can change this to your own domain
        //     any time. Example:
        //
        //         AllowedOrigins::some_exact(&["https://example.com"]);
        //     or
        //         AllowedOrigins::some_regex(&["^https://(.+)\.example\.com$"]);
        //
        //     For more examples (or examples don't work): https://docs.rs/rocket_cors
        //
        //     Additionaly, domain(s) can probably be configured by adding additional variables in the .env
        //     (startup environment variables) or Rocket.toml, but then this part of the code will become
        //     more complex.  @Robustness @HelpNeeded
        allowed_origins: AllowedOrigins::all(),
        // Note(andrew): We are only using GET and POST right now, but if you add anything, remember to look
        //     at this. On the other note, is it bad that our endpoints have either GET or POST, because this
        //     probably means that we accept 'GET' AND 'POST' *everywhere*?  @Robustness @Question
        allowed_methods: vec![Method::Get, Method::Post]
        // Note(andrew): Notice all the transformation that is done here. The thing is that rocket_cors casts
        //     rocket::http::Method(s) into their internal structure type, and also it is using hashset instead
        //     of simple array. So to make this easier, first we write out our wanted method above in a 'vector'
        //     and then using this code to iterate over it, mapping each instance of a Method into the struct
        //     rocket_cors expects (From::from will call trait that is implemented by the rocket_cors itself!),
        //     and finally, '.collect()' allows to convert iterator into expected type (hashset-like structure
        //     in the case of rocket_cors).
        .into_iter().map(From::from).collect(),
        // Note(andrew): Since we are handling basicauth with an external webserver (alongside with SSL certs
        //     and domain stuff) as our reverse proxy, I'm not sure that we actually need this. But anyway, we
        //     probably do, so don't remove this for now, even though there is not authorization in the source
        //     code itself (it is in the caddy config on production). This is optional anyways, so should be ok
        //     to never remove this.  @Robustness
        //
        //     You might want:
        //
        //         AllowedHeaders::some(&["Authorization", "Accept"])
        //
        //     But this blocks (returns 403) any request that has any other header, not specified in the list,
        //     so this seems not very useful (or, at least, very annoying to configure) in our case. Instead,
        //     we just allow everything for now:  @Robustness
        allowed_headers: AllowedHeaders::all(),
        allow_credentials: true,
        ..Default::default()
    }
    // There is some magic going on here, converting our config into a rocket fairing.  @Robustness
    .to_cors().unwrap();

    rocket::build()
        // Register our endpoints with /api/ root prefix.
        .mount("/api", routes![
            endpoints::post_klocc_job,
            endpoints::get_health,
        ])
        // Managing cache mutex. This allows rocket to pass this instance to us in any handler where we need
        // it, using rocket's internal 'State' wrapper.
        .manage(data::init_db())
        // Adding CORS middleware.
        .attach(cors)
}

