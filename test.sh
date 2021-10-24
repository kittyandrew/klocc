curl -sX POST -H "Content-Type: application/json" -d "{\"username\": \"$1\", \"reponame\": \"$2\", \"provider\": \"${3:-github}\"}" 0.0.0.0:8080/api/jobs
