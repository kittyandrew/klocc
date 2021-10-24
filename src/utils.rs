

// This function takes service name, and other useful arguments, and expands them into
// a valid url for git repository for specific service provider. In theory, we could
// just allow passing url, but it's questionable decision from the security standpoint.
pub fn expand_url(service: &String, username: &String, reponame: &String) -> Result<String, String> {
    match service.as_str() {
        "github" => Ok(format!("https://github.com/{}/{}.git", username, reponame)),
        "gitlab" => Ok(format!("https://gitlab.com/{}/{}.git", username, reponame)),
        _        => Err(format!("Service provider for git with a name '{}' is not supported!", service))
    }
}

