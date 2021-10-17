use rocket::serde::json::{json, Value};
use std::time::SystemTime;
use rocket::tokio::task;
use rocket::State;

use crate::counter::{get_data_from_repo, get_latest_hash};
use crate::utils::expand_url;
use crate::body::PostJobData;
use crate::data::Database;


// Note(andrew): Since we now store creation time for each data instance in the
//     cache, we can add additional "anti-ddos" condition for /jobs the endpoint,
//     in which we check if cache is newer than 'TRUST_CACHE_SECONDS', and if it
//     is, we do not need to make a request via git to get the latest valid hashes
//     from the remote repository. So, for example, someone who spams a bunch of
//     requests for the same repository will recieve faster (instant) response and
//     won't trigger creating new verification thread for each request. Just make
//     sure to keep this low enough that realistically nobody have a chance to be
//     confused and get the bad result.
const TRUST_CACHE_SECONDS: u64 = 60 * 15;  // @Robustness: No-doubt cache for 15 minutes, will it cause issues?


/*
   First, we are trying to pull data from the cache (in-memory database for now),
   for now this will be a single unique entry per repository (only github), since
   we are not allowing to pass any target branch -- only the default branch -- for
   analysis.

   If we find anything in the cache, we are happy to instantly respond with existing
   data.

   TODO(andrew): This implementation has several obvious issues:

     [x] We are assuming github as the service provider for repository origin.

     [x] We don't verify that repository is valid, before creating analyzer thread.

     [x] Cache is never invalidated (other than reload), which means if we analyzed a
         repository some time ago, and it's still in-memory, we yield invalid results,
         even though they are probably pretty close, at first (still pretty bad).

     [ ] Current implementation is just holding request open until we are done. This is
         arguably a flawed solution. Below I describe alternative implementation ideas,
         all of which would be important to at least carefully consider:

             - Switch to job queue, where we don't hold the api request, but instead just
               dispatch task, and return task id immediately. Then frontend can query job
               status every N seconds, and get job status. Sounds pretty reasonable, but
               the obvious flip-side of this implementation is forcing frontend to run a
               busy loop, spamming requests to the server for relevant data. Callback is
               not feasable due to the fact that this application targets frontends like
               webpage-applications running on the client side.

             - Persistent connection with frontend through websockets, where we can send
               status events after receiving payload from the front-end. And 'done' event
               with data would be just another type of event. This complicates things a
               little bit, because websockets require persistent connection and stuff like
               that.

         Note(andrew): If this is actually something we are going to implement (anything of
             the above), there might be a growing need for persistent storage (see below).

     [?] We are not allowing to choose target branch, so the database, aka caching,
         needs to work properly with branches later.

     [?] No persistent storage is being used. Note(andrew): I don't think we need any.
*/


// This endpoint is designed to help monitor and debug service availability. It
// is used in 'healthcheck' routine for docker container daemon, and it is also
// useful for getting some status information about cache storage. Note that we
// don't have 'format' (content-type header) required for this endpoint (or any
// other header requirements), so *any* GET request has to be valid here.
#[get("/health")]
pub async fn get_health(db: &State<Database>) -> Value {
    // Just for informational purposes add count of total cached items
    // in the storage to the response (TODO(andrew): add storage size,
    // meaning an actual amount of memory taken by cache).
    let count = db.lock().await.len();
    return json!({
        "status": 200, "message_code": "info_health_ok", "message": "KLOCC is healthy!",
        "data": {"cached_count": count},
    })
}


#[post("/jobs", format = "application/json", data = "<data>")]
pub async fn post_klocc_job(db: &State<Database>, data: PostJobData) -> Value {
    // Note(andrew): First thing first, we are trying to expand service name into url, using our
    //     helper function. If it fails to match provider to any known service, it returns an error
    //     message, explaining the problem, which we pass through json directly to the callee. To
    //     see/update the list of supported providers for check the source of 'expand_url', that's
    //     the only place where we pattern match it.
    let repo_url = match expand_url(&data.provider, &data.username, &data.reponame) {
        Ok(value) => value,
        Err(msg)  => return json!({ "status": 400, "message_code": "err_bad_service", "message": msg })  // Early return from the handler.
    };

    // TODO(andrew): Since we are getting 'data' here, store it outside the code block, because
    //     we want to query it again later. Or should we still read it from mutex (sounds like
    //     some potential race conditions regarding parallel-processed requests are possible, or
    //     even 'to be expected', so better to think carefully about this)?  @Robustness @Speed

    // Note(andrew): Before requesting latest hash from the remote, let's check if the data is
    //     present in the cache, and if it is, we can check if it is recent enough (was created
    //     less than 'TRUST_CACHE_SECONDS' ago), which means we can immediately return, as we are
    //     not concerned enough about validity of the that data to take time for additional meta-
    //     data request. Hopefully, this increases our robustness and allows to survive situations
    //     like DoS (either intentional or just an unexpected amount of load).
    {
        let guard = db.lock().await;  // It is important for us that this lock will be freed after the code block.
        let result = guard.get(&repo_url);

        if result.is_some() {
            let data = result.unwrap();  // @SafeUnwrap: Data must be present, because we just checked for 'some'.
            let curr = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();  // Get current system time. @UnsafeUnwrap @Robustness

            if (data.creation_time + TRUST_CACHE_SECONDS) >= curr.as_secs() {
                return json!({
                    "status": 200, "message_code": "info_success_cached_recent",
                    "message": "Your request was satisfied instantly, because it was found in cache.",
                    "data": data,
                });  // Early return from the handler.
            }
        }
    }

    let hash: String;
    {
        // Preparing values to move into the thread.
        let _repo_url = repo_url.clone();
        let _target   = "HEAD".to_string();
        // This method will return a hash of the latest commit in the repository for us to save for later,
        // or an error if the repository doesn't exist (or it's not available).
        hash = match task::spawn_blocking(move || get_latest_hash(_repo_url, _target)).await.unwrap() {  // @PotentialPanic @Robustness: Thread can fail?
            Ok(value) => value,
            Err(msg)  => return json!({"status": 400, "message_code": "err_failed_to_fetch_from_repo", "message": msg})  // Early return from the handler.
        };
    }

    {
        // Note(andrew): Here we are asynchronously waiting on the lock, if the resource is busy,
        //     and otherwise we just grab the guard and lock data storage. Then we are looking for
        //     our repository in the cache, and if it is present (the result of .get is not empty),
        //     we are passing grabbed data to write it back into the response to the callee, if it
        //     is relevant. Relevancy is determined by checking if the latest hash at the HEAD of
        //     the repository macthes stored hash. If it does not match, we know that new commits
        //     have been pushed to the repository since our last analysis, so there is some chance
        //     (although we don't know, and probably don't have a way to know, exactly) that stored
        //     result is inaccurate.
        let guard = db.lock().await;  // It is important for us that this lock will be freed after the code block.
        let result = guard.get(&repo_url);

        if result.is_some() {
            let data = result.unwrap();  // @SafeUnwrap: Data must be present, because we just checked for 'some'.
            // Verify that hash matches since the last time we ran the klocc job.
            if data.hash == hash {
                return json!({
                    "status": 200, "message_code": "info_success_cached",
                    "message": "Your request was satisfied instantly, because it was found in cache.",
                    "data": data,
                });  // Early return from the handler.
            }
        }
    }

    {
        // Note(andrew): Here we are copying values from our input strings, because we are going to
        // pass them down into the thread, which will own them from now on (but we might want to use
        // original values later in the code). 
        let _username = data.username.clone();
        let _reponame = data.reponame.clone();
        let _repo_url = repo_url.clone();

        // Note(andrew): Here we are using high-level tokio API for dispatching synchronous tasks in
        //     asynchronous manner, by 'moving' them into a newly spawned thread and awaiting until it
        //     finishes (wait is asynchronous). Which, in practice, means that the server can process
        //     other requests in the meantime and do other useful work, while we are waiting for download
        //     or result of analysis (everything inside dispatched routine below).
        let result = task::spawn_blocking(move || get_data_from_repo(_username, _reponame, _repo_url)).await.unwrap();  // @PotentialPanic @Robustness: Thread can fail?

        // Note(andrew): Our klocc procedure returns a result, where different errors and edge-cases are
        //     handled, explained and propagated in a form of an error message (as a string), so here we
        //     are doing a check for that in our result. If we confirmed that this is indeed an error,
        //     unpack the error message and pass it directly back to the callee.
        if result.is_err() {
            return json!({ "status": 500, "message_code": "err_counter_failed", "message": result.err() });  // Early return from the handler.
        }

        let mut data = result.unwrap();  // @SafeUnwrap: Data must be present, because we just checked for the error.
        data.hash = hash;
        // Note(andrew): Await and lock temporary mutex guard value, that is being used immediately in-place
        //     to insert values into the cache, and then, in the next step after insert, the guard is freed
        //     and the cache storage is unlocked for other parallel running jobs.
        db.lock().await.insert(repo_url.clone(), data);  // We are safe to unwrap here, because we check that value is ok right above.
    }

    // Note(andrew): Lock the guard temporarily here, as we are going to query database for our data
    //     reference, and safely unwrap it directly into the json, because we know it must be present,
    //     as we just added it right above this code block (we only reach here after adding new data).
    let guard = db.lock().await;
    return json!({
        "status": 200, "message_code": "info_success_generated",
        "message": "The repo was analyzed successfully and result was stored for later reference.",
        "data": guard.get(&repo_url).unwrap(),  // @SafeUnwrap: Data must be present, because we inserted it previously.
    });
}
