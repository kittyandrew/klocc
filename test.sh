curl -sX POST -H "Content-Type: application/json" -d "{\"username\": \"$1\", \"reponame\": \"$2\", \"provider\": \"github\"}" 0.0.0.0:8080/api/jobs

: '
  An example request for *this* repository would be (note pipe in the end):

    curl -sX POST -H "Content-Type: application/json" -d "{\"username\": \"kittyandrew\", \"reponame\": \"klocc\", \"provider\": \"github\"}" 0.0.0.0:8080/api/jobs

  And the response should be (note the hashes and exact numbers might not match):  @KeepUpToDate

  P.S. This data has undergone a non-trivial manual formatting. Original response containes no extra spaces or new lines.

    {
        "message": "Your request was satisfied instantly, because it was found in cache.",
        "message_code": "info_success_cached",
        "status": 200,
        "data": {
            "hash":"d0832589bbf359e187706f57d9838f8d97ec9c57",
            "languages":{
                "Dockerfile":{
                    "files": {"Dockerfile":{"blanks":2,"code":13,"comments":6}},
                    "total": {"blanks":2,"code":13,"comments":6}
                },
                "Rust":{
                    "files": {"src/body.rs":{"blanks":9,"code":29,"comments":14},"src/counter.rs":{"blanks":33,"code":84,"comments":36},"src/data.rs":{"blanks":17,"code":44,"comments":3},"src/endpoints.rs":{"blanks":13,"code":55,"comments":83},"src/main.rs":{"blanks":5,"code":12,"comments":7},"src/utils.rs":{"blanks":3,"code":6,"comments":3}},
                    "total": {"blanks":80,"code":230,"comments":146}
                },
                "Shell":{
                    "files": {"test.sh":{"blanks":0,"code":1,"comments":0}},
                    "total": {"blanks":0,"code":1,"comments":0}
                },
                "TOML":{
                    "files": {"Cargo.toml":{"blanks":3,"code":8,"comments":1},"Rocket.toml":{"blanks":0,"code":3,"comments":0}},
                    "total": {"blanks":3,"code":11,"comments":1}
                },
                "YAML":{
                    "files": {"docker-compose.yml":{"blanks":1,"code":11,"comments":2}},
                    "total": {"blanks":1,"code":11,"comments":2}
                }
            },
            "repo":  "https://github.com/kittyandrew/klocc.git",
            "total": {"blanks":86,"code":266,"comments":155}
        }
    }

'
