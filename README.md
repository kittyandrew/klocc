# KLOCC (Kitty Lines Of Code Counter)
This service allows you to request a detailed information regarding lines of code/comments/blanks in the git repository (_at the moment, only github and gitlab are allowed_).  
  
Check out `test.sh` ([click me](./test.sh)) to see example request and expected response.

## Packaging

Nix is the source of truth for builds:

- `nix build .#klocc` builds the backend binary.
- `nix build .#klocc-frontend` builds the frontend assets.
- `nix build .#docker-image` builds the Docker image tarball.

The Docker Compose setup was removed; deployment should consume the Nix-built image output and define service/network policy in the deployment environment.

## Binary cache

Builds are published to `cache.kittyandrew.dev`, so Nix can download these outputs instead of rebuilding them.

On NixOS:

```nix
nix.settings = {
  extra-substituters = ["https://cache.kittyandrew.dev/nix-cache"];
  extra-trusted-public-keys = ["cache.kittyandrew.dev-1:yy5fdErj1riKOjND10kzD5mp0L8/C8RFG3VkMizhGg4="];
};
```

Elsewhere, in `~/.config/nix/nix.conf` (or `/etc/nix/nix.conf` for all users):

```
extra-substituters = https://cache.kittyandrew.dev/nix-cache
extra-trusted-public-keys = cache.kittyandrew.dev-1:yy5fdErj1riKOjND10kzD5mp0L8/C8RFG3VkMizhGg4=
```

The `extra-` prefixes append rather than replace, so `cache.nixos.org` keeps working. The cache is read-only
and needs no credentials; it serves only what this repository's flake builds.

## WebApp Implementations
- [klocc-frontend](https://github.com/Katerynaru4/klocc-frontend) (`official`)
