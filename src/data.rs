use rocket::serde::{Serialize, Deserialize};
use rocket::tokio::sync::Mutex;
use std::collections::HashMap;


#[derive(Serialize, Deserialize, Debug)]
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


#[derive(Serialize, Deserialize, Debug)]
#[serde(crate = "rocket::serde")]
pub struct LanguageInfo {
    pub total: Info,
    pub files: HashMap<String, Info>,
}


impl LanguageInfo {
    pub fn new(total: Info) -> Self {
        Self { total: total, files: HashMap::new() }
    }
}


#[derive(Serialize, Deserialize, Debug)]
#[serde(crate = "rocket::serde")]
pub struct Data {
    pub repo: String,
    pub hash: String,
    // pub branch: String,
    pub total: Info,
    pub languages: HashMap<String, LanguageInfo>,
}


impl Data {
    pub fn new(repo: String, total: Info) -> Self {
        Self { repo: repo, total: total, languages: HashMap::new(), hash: "".to_string() }
    }
}


pub type Database = Mutex<HashMap<String, Data>>;


pub fn init_db() -> Database {
    let storage = HashMap::<String, Data>::new();
    Mutex::new(storage)
}

