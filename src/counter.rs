use std::time::{SystemTime, UNIX_EPOCH};
use tokei::{Config, Languages};
use std::process::{Command};
use tempdir::TempDir;

use crate::data::{Info, LanguageInfo, Data};


// Logging with current unix timestamp. Useful to reduce number of typed lines to do basic logging.
macro_rules! info {
    ( $s:tt, $( $x:expr ),* ) => {
        {
            let start_time = SystemTime::now();
            let curr_time  = start_time.duration_since(UNIX_EPOCH).unwrap();
            let msg = format!($s, $( $x, )*);
            println!("[{:>10}] - {}", curr_time.as_secs(), &msg);
        }
    };
}


pub fn get_latest_hash(repo_url: String, branch: String) -> Result<String, String> {
    // Call git command to fetch latest hash for the given branch from the remote repository.
    let output = Command::new("git")
        .args(["ls-remote", &repo_url, &branch])
        .output();

    // Note(andrew): Confusingly enough, this is an internal rust error for running
    //     command. Meaning, if command ran at all, any exit code will return *result*
    //     here. I guess this can fail only if rust couldn't open a shell process,
    //     or something like that.
    if output.is_err() {
        return Err(format!("Internal error while executing command: {:?}", output.err()));
    };

    let result = output.unwrap();

    // Note(andrew): Here we are doing an actual check for what status the command
    //     has returned, and return an error from here if the command didn't finish
    //     successfully, where success is defined by whether process returned 0 as
    //     its exit status code.
    if !result.status.success() {
        return Err(format!("Failed to fetch latest hash from the remote repository ({}) for the branch '{}': process returned non-zero exit status code.", &repo_url, &branch));
    };

    // Take stdout and cut first string before \t, which is a hash, in the case of 'git ls-remote'.
    let result_string = match String::from_utf8(result.stdout) {
        Ok(value) => value,
        Err(msg)  => return Err(format!("Invalid UTF-8 sequence: {}", msg))
    };

    match result_string.split("\t").next() {
        Some(v) => return Ok(v.to_string()),  // Return successfully.
        None    => return Err("Failed to split result by '\t' to extract hash from the 'git ls-remote'!".to_string())
    };
}


pub fn get_data_from_repo(username: String, reponame: String, repo_url: String) -> Result<Data, String> {
    //let branch   = "master";
    let branch   = "default";  // Not a real branch, just used for logging rn.

    info!("Starting KLOCC procedure for {} ({})", &repo_url, &branch);

    // Generate new random temporary directory.
    let dir = match TempDir::new("cloned_repositories") {
        Ok(value) => value,
        Err(e)    => return Err(format!("Failed to create temporary directory: {:?}!", e))
    };

    // Generating full path from the random temporary directory to repository project,
    // using real project name, which we will strip later.  @Speed?
    let repo_dir  = dir.path().join(&reponame);
    let repo_path = repo_dir.to_str().unwrap();

    info!("Cloning {} ({}) ...", &repo_url, &branch);

    {  // This might be worth factoring out in the separate function.
        let output = Command::new("git")
            .args(["clone", "--depth", "1", "--single-branch", "--recurse-submodules", &repo_url, &repo_path])
            .output();

        // Note(andrew): Confusingly enough, this is an internal rust error for running
        //     command. Meaning, if command ran at all, any exit code will return *result*
        //     here. I guess this can fail only if rust couldn't open a shell process,
        //     or something like that.
        if output.is_err() {
            return Err(format!("Internal error while executing command: {:?}", output.err()));
        };

        // Note(andrew): Here we are doing an actual check for what status the command
        //     has returned, and return an error from here if the command didn't finish
        //     successfully, where success is defined by whether process returned 0 as
        //     its exit status code.
        if !output.unwrap().status.success() {
            return Err("Failed to fetch the repository: process returned non-zero exit status code.".to_string());
        };
    }

    info!("Counting lines for {} ({}) ...", &repo_url, &branch);

    // The paths to search. Accepts absolute, relative, and glob paths.
    let paths    = &[&repo_path];
    // Exclude any path that contains any of these strings.
    let excluded = &[];
    // Config allows you to configure what is searched and counted. Defaulting all un-filled
    // fields to default values from the config.
    // Refer to: https://docs.rs/tokei/12.1.2/tokei/struct.Config.html
    let config   = Config { treat_doc_strings_as_comments: Some(true), ..Config::default() };

    // Here we are calling the 'tokei' lib to actually read given paths and provide us with
    // statistical information about it.
    let mut languages = Languages::new();
    languages.get_statistics(paths, excluded, &config);

    let total    = languages.total();
    let mut info = Info::new(total.code as u32, total.comments as u32, total.blanks as u32);
    let mut data = Data::new(repo_url.clone(), info);

    // These variables will be used in a language loop.
    let mut lang:   LanguageInfo;
    let mut name:   String;
    let mut offset: usize;

    for (key, item) in languages.iter() {
        info = Info::new(item.code as u32, item.comments as u32, item.blanks as u32);
        lang = LanguageInfo::new(info);

        for report in item.reports.iter() {
            info = Info::new(report.stats.code as u32, report.stats.comments as u32, report.stats.blanks as u32);

            // Convert path buffer item into 'str' first, and then into string for manipulation.
            name = report.name.to_str().unwrap().to_string();
            // Calculate offset of the temp dir prefix + repository name + length of '/' (which is
            // a last slash, that is present after the repo name). Then use '.drain', which eats
            // 'name' string up to the point of 'offset'. Maybe there is more straightforward way
            // to do this, idk.
            offset = name.find(&reponame).unwrap() + reponame.len() + 1;
            name.drain(..offset);

            lang.files.insert(name, info);
        }

        data.languages.insert(key.to_string(), lang);
    }

    info!("Cleaning up after {} ({}) ...", &repo_url, &branch);

    dir.close().unwrap();

    return Ok(data);
}

