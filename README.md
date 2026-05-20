# KLOCC (Kitty Lines Of Code Counter)
This service allows you to request a detailed information regarding lines of code/comments/blanks in the git repository (_at the moment, only github and gitlab are allowed_).  
  
Check out `test.sh` ([click me](./test.sh)) to see example request and expected response.

## Packaging

Nix is the source of truth for builds:

- `nix build .#klocc` builds the backend binary.
- `nix build .#klocc-frontend` builds the frontend assets.
- `nix build .#docker-image` builds the Docker image tarball.

The Docker Compose setup was removed; deployment should consume the Nix-built image output and define service/network policy in the deployment environment.

## WebApp Implementations
- [klocc-frontend](https://github.com/Katerynaru4/klocc-frontend) (`official`)
