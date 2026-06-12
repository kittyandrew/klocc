# Rust Perf Profiling CLI

- Use Linux `perf` for this repo; `samply` is blocked here by `perf_event_paranoid=2` unless the host is reconfigured.
- Build first so profiles do not include compile time:

```sh
CARGO_PROFILE_RELEASE_DEBUG=true nix develop -c cargo build --release -p <crate>
```

- Preferred sampled profile for scanner internals:

```sh
home=$(mktemp -d /tmp/opencode/klocc-prof-home.XXXXXX)
out=$(mktemp -d /tmp/opencode/klocc-prof-run.XXXXXX)
HOME="$home" nix shell nixpkgs#perf -c perf record \
  --call-graph dwarf,65528 -F 199 -o "$out/perf.data" -- \
  timeout 90s ./target/release/klocc \
  scan .#packages.x86_64-linux.default --out "$out/scan.sqlite"
```

- Do not set fresh `XDG_CACHE_HOME` unless intentionally profiling cold Nix flake/git cache behavior; it can make the profile mostly `git` pack indexing.
- Do not profile `cargo run` for scanner internals unless Cargo caches are warm and build is already complete; direct `target/release/...` is cleaner.
- If using the canonical Cargo form, use it like this after a release build:

```sh
nix shell nixpkgs#perf -c perf record --call-graph dwarf,65528 -F 199 -o /tmp/opencode/perf.data -- \
  cargo run --release -p klocc -- scan <root> --out <artifact.sqlite>
```

- Summarize non-interactively:

```sh
nix shell nixpkgs#perf -c perf report -i /tmp/opencode/perf.data --stdio --no-children --percent-limit 0.5 --sort comm,dso,symbol
nix shell nixpkgs#perf -c perf report -i /tmp/opencode/perf.data --stdio --children --percent-limit 1 --sort symbol,dso
```

- Use `perf stat` for wall/counter measurements:

```sh
nix shell nixpkgs#perf nixpkgs#time -c perf stat -d -- ./target/release/klocc check /tmp/klocc-self.sqlite
```

- Enable scanner phase timings with `KLOCC_SCAN_TIMINGS=1`:

```sh
KLOCC_SCAN_TIMINGS=1 ./target/release/klocc scan .#packages.x86_64-linux.default --out /tmp/scan.sqlite
```

- For GUI, run the live Wayland stack with `target/release/klocc-gui`, then attach to the GUI PID:

```sh
nix run .#gui-wayland-live -- start /tmp/klocc-self.sqlite
source /tmp/opencode/klocc-gui-live/env
nix shell nixpkgs#perf -c perf record --call-graph dwarf,65528 -F 499 \
  -o /tmp/opencode/gui.perf.data -p "$KLOCC_GUI_PID" -- sleep 12
```

- While attached, drive real interactions with `klocc-gui-wayland-live click ...` or `xdotool`; then inspect `gui.log`, screenshots, and `perf report`.
- Expect GUI server profiles in this environment to be dominated by Lavapipe/llvmpipe software rasterization; app-side hotspots are meaningful only after filtering to `comm=nix-system-tree`.
- Current scanner finding: after replacing linear dependency dedupe with a `HashSet`, routing vendored Cargo LOC through the persistent cache, stamp-based source rollups, and reading derivers/references from `nix path-info --json`, a fully warm self-scan is about 3.2-3.7s wall; remaining time is mostly Nix subprocess startup/derivation JSON and SQLite writes.
- Current GUI finding: storing `Artifact` behind `Rc` avoids cloning all source records into render closures; remaining sampled CPU is mostly software Vulkan rasterization.
