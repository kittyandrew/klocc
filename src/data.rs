use rocket::tokio::sync::Mutex;
use std::collections::HashMap;
use rocket::serde::Serialize;
use std::time::SystemTime;


#[derive(Serialize, Debug)]
#[serde(crate = "rocket::serde")]
pub struct Info {
    pub code: u32,
    pub comments: u32,
    pub blanks: u32,
}


impl Info {
    pub fn new(code: u32, comments: u32, blanks: u32) -> Self {
        Self { code: code, comments: comments, blanks: blanks }
    }
}


#[derive(Serialize, Debug)]
#[serde(crate = "rocket::serde")]
pub struct FileInfo {
    pub path: String,
    pub code: u32,
    pub comments: u32,
    pub blanks: u32,
}


impl FileInfo {
    pub fn new(path: String, code: u32, comments: u32, blanks: u32) -> Self {
        Self { path: path, code: code, comments: comments, blanks: blanks }
    }
}


#[derive(Serialize, Debug)]
#[serde(crate = "rocket::serde")]
pub struct LanguageInfo {
    pub name: String,
    pub total: Info,
    pub files: Vec<FileInfo>,
}


impl LanguageInfo {
    pub fn new(name: String, total: Info) -> Self {
        Self { name: name, total: total, files: Vec::new() }
    }
}


#[derive(Serialize, Debug)]
#[serde(crate = "rocket::serde")]
pub struct Data {
    pub creation_time: u64,
    pub repo: String,
    pub hash: String,
    // pub branch: String,
    pub total: Info,
    pub languages: Vec<LanguageInfo>,
}


impl Data {
    pub fn new(repo: String, total: Info) -> Self {
        // Note(andrew): Getting current timestamp in seconds from system clock here,
        //     which might be used later for checking whether our cached data is very
        //     recent or anything else. The type of 'SystemTime' duration is u64, so
        //     we are safe from the future overflows.
        //
        //     Still, the fact that we *unsafely* unwrap here is probably problematic.
        //     I am not sure why would 'SystemTime' fail, and if/when it does there are
        //     likely other critical parts of the system (host machine) that will/already
        //     failed, so this will not affect things in the overall picture.  @Robustness
        let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();

        Self {
            creation_time: now.as_secs(),
            repo: repo, total: total,
            languages: Vec::new(),
            hash: "".to_string(),
         }
    }
}


pub type Database = Mutex<HashMap<String, Data>>;


// A helper function to create an empty instance of the hashmap-mutex structure, which
// is used as in-memory storage.
pub fn init_db() -> Database {
    let storage = HashMap::<String, Data>::new();
    Mutex::new(storage)
}

