
curl -sX POST -H "Content-Type: application/json" \
    -d "{\"username\": \"${1:-kittyandrew}\", \"reponame\": \"${2:-klocc}\", \"provider\": \"${3:-github}\"}" \
    0.0.0.0:8080/api/jobs

