# Validation Worklog

Purpose: capture cross-project validation runs, weird findings, open questions, and follow-up fixes while testing `klocc`, `klocc-gui`, and `kloccd` on non-self inputs.

## May 23, 2026

### Target: `github:kittyandrew/waybap#packages.x86_64-linux.default`

Status: scanner, artifact validation, and GUI exercise passed.

Evidence:

- `nix build --no-link --print-out-paths -L github:kittyandrew/waybap#packages.x86_64-linux.default` completed and produced `/nix/store/ibway2pscv0zcy73am5968sc3w0gjc7i-waybap-0.1.0`.
- A direct scan of the realized store path reached `source.init_derivations` quickly, then spent multiple minutes in the next phase without emitting more progress.
- The scanner process remained CPU-active with multiple worker threads and only `loc-cache.sqlite` open, which points at source graph traversal/LOC measurement rather than Nix build realization.
- Follow-up scanner reruns initially did not reach scanner execution: the first ad-hoc Rust shell lacked `cc`; retries with `stdenv.cc` then failed with `No space left on device`.
- After root/Nix space was restored, the stale realized path had been garbage-collected and failed with `path ... is not valid`; re-realizing the flake produced `/nix/store/ibway2pscv0zcy73am5968sc3w0gjc7i-waybap-0.1.0` again.
- Direct scan with `target/debug/klocc` completed successfully and wrote `/tmp/opencode/klocc-waybap-retry.sqlite` (`23M`).
- `target/debug/klocc check /tmp/opencode/klocc-waybap-retry.sqlite` passed: schema version 1, 5 store paths, 1671 source units, 0 unknown derivation sources, 0 generated derivation outputs, graph integrity passed.
- Timings: `scan.total 113136.389ms`, `scan.source_graph 109263.759ms`, `source.nix_graph 108239.634ms`, `scan.write 2393.927ms`.
- The added progress logs confirmed the prior silent period was source realization/LOC measurement. Slow examples: `vendor-registry` measurement took about 5.27s; Rust toolchain tarballs took about 1.5s to 3.15s each.
- Artifact stats showed a tiny runtime closure (`5` paths, `33.9 MiB`) but a huge full build-source graph (`1671` source units, `405444529` total LOC, `254724292` code LOC). Top LOC contributors were Rust, Linux, LLVM, GCC, Boost, and Cargo vendor sources from build-time dependencies.
- Running raw `target/debug/klocc-gui` failed with `libxcb.so.1` missing because the binary needs the dev shell GUI library environment.
- `nix develop -c env KLOCC_GUI_EXERCISE=1 target/debug/klocc-gui /tmp/opencode/klocc-waybap-retry.sqlite` passed: loaded 1671 sources in 283.81ms; 960 cold layouts averaged 1.378ms; max 1455 rects; cache probes averaged 0.000898ms; hit/drill completed in 0.122ms.

Open questions:

- Does `source.nix_graph` scale acceptably for package-sized real projects, given waybap needs about 109s for source graph generation on a cold-ish run?
- Should the default stats/UI emphasize runtime-linked source first, since full build-source LOC can make a tiny runtime package look like a 405M LOC artifact?

Immediate follow-up:

- Review whether `ensure_derivation_sources` can add the same source candidate repeatedly before builder-level de-duplication/cache catches it.

### Target: `.#docker-image` / `.#kloccd-server-image`

Status: build and runtime validation passed.

Evidence:

- Both names resolve to the same `kloccd-server` Docker image derivation.
- A duplicate alias build was accidentally started; it completed far enough to verify the derivation graph and `kloccd` dependency build path, then the retained PTY was killed after completion notification.
- Canonical `nix build .#docker-image -L` completed successfully. Final line: `docker-image-kloccd-server.tar.gz> Finished.`
- `kloccd` built and ran its crate test target during the Nix build. Result: `test result: ok. 0 passed; 0 failed`.
- The only real log anomaly was `error (ignored): SQLite database ... eval-cache ... is busy`, caused by concurrent Nix evaluations in this validation session.
- `docker load -i result` loaded `kloccd-server:0.1.6`.
- Running the container and requesting `http://127.0.0.1:18080/api/health` returned `{"data":{"cached_count":0},"message":"KLOCC is healthy!","message_code":"info_health_ok","status":200}`.

Open questions:

- Should the flake keep both image aliases, or does the alias duplication make validation and user documentation noisier than useful?
- Should validation scripts avoid concurrent `nix build`/`nix develop` invocations because Nix eval-cache contention makes logs noisy and can delay unrelated runs?

Immediate follow-up:

- Consider replacing the duplicate image aliases with one documented name if alias noise continues to confuse validation.

### Target: `nixpkgs#hyprland`

Status: scanner, artifact validation, and GUI exercise passed.

Evidence:

- `nix build --no-link --print-out-paths -L nixpkgs#hyprland` completed from cache and produced `/nix/store/nyfjvjg4qpssa5513q41xjvrmlhz16gs-hyprland-0.55.1`.
- No real build errors/warnings were found; `libxcb-errors` only matched the word `error` in a package name.
- Scanner rerun did not reach scanner execution: the first ad-hoc Rust shell lacked `cc`; subsequent retries were blocked by root/Nix store space exhaustion.
- After root/Nix space was restored, the previous realized path had been garbage-collected and had to be re-realized.
- Initial Hyprland scan completed, but stats exposed a real labeling bug: 28 runtime-linked source units were labeled `unknown-source / unknown-deriver` even though only one runtime path actually had an unknown deriver.
- Fix 1 changed known derivations with no direct source candidate into generated/no-source-candidate classifications instead of unknown-deriver.
- Fix 2 improved merge precedence so better source classifications replace older `unknown-source / unknown-deriver` units with the same source-unit key.
- Fix 3 added `unknown-derivation-source / derivation-unavailable` for known deriver paths whose `.drv` is no longer valid/inspectable in the local store.
- Final fixed scan wrote `/tmp/opencode/klocc-hyprland-fixed3.sqlite` and completed in `7390.895ms` with warm LOC cache.
- `target/debug/klocc check /tmp/opencode/klocc-hyprland-fixed3.sqlite` passed: 270 store paths, 1703 source units, 9 unknown derivation sources, 289 generated derivation outputs, graph integrity passed.
- Remaining source-health unknowns are correctly split: 9 `unknown-derivation-source / derivation-unavailable` entries for known-but-uninspectable `.drv` paths, and 1 `unknown-source / unknown-deriver` for `strip.sh`.
- `nix develop -c env KLOCC_GUI_EXERCISE=1 target/debug/klocc-gui /tmp/opencode/klocc-hyprland-fixed3.sqlite` passed: loaded 1703 sources in 287.63ms; 960 cold layouts averaged 1.239ms; max 1541 rects; cache probes averaged 0.000723ms; hit/drill completed in 0.105ms.

Open questions:

- Should `klocc check` distinguish `derivation-unavailable` from true unknown derivers more explicitly in its summary wording?
- Should full build-source LOC be hidden behind a filter by default for desktop packages, since Hyprland maps to 503M total source LOC when all build-time dependencies are included?

Immediate follow-up:

- None for this target; Zed remains as the larger validation target.

### Target: Zed project

Status: prerequisite build blocked before scanner validation.

Evidence:

- `/tmp/opencode/zed-project` shallow clone completed.
- `nix flake show github:zed-industries/zed --json` confirmed `packages.x86_64-linux.default` exists.
- The factory environment ignored Zed's untrusted flake `extra-substituters` and `extra-trusted-public-keys`, so this build attempted substantially more local/cache-untrusted work than a developer machine with Zed Cachix trusted.
- `nix build --no-link --print-out-paths -L /tmp/opencode/zed-project#packages.x86_64-linux.default` failed before scanner validation. Primary failure: root/Nix store filled up (`No space left on device`), including Nix DB writes failing with `database or disk is full`.
- After root/Nix space was restored, Zed was recloned under `/var/lib/factory/workspaces/zed-validation/zed-project` to avoid root-backed `/tmp` checkout pressure.
- `nix build --dry-run --no-link -L /var/lib/factory/workspaces/zed-validation/zed-project#packages.x86_64-linux.default` succeeded as a dry run but reported 2425 derivations to build and 950 paths to fetch (`1.9 GiB` download, `8.1 GiB` unpacked).
- Current free space after the dry run is about `24G` on `/`/`/nix` and `54G` on `/var/lib/factory`; a full Zed build is still too risky in this environment without trusted binary caches or more Nix store headroom.

Open questions:

- Does a large Rust/GPUI workspace produce usable Cargo source attribution without overwhelming the SQLite artifact or native viewer?
- Does local flake source fallback correctly identify the cloned Zed workspace as the root source when scanning a local flake output?
- Should large third-party validation targets document cache trust requirements separately from scanner behavior, so a slow prerequisite build is not mistaken for a scanner issue?
- Should Zed-scale validation run only on a machine with trusted Zed caches and enough Nix store space?

Immediate follow-up:

- Do not retry the full Zed package build in this environment until cache trust is addressed or substantially more Nix store space is available.
- If retried, keep the checkout under `/var/lib/factory` rather than root-backed `/tmp`.

### Environment Blocker: Root/Nix Store Space

Status: recovered for normal project validation; still insufficient for full Zed prerequisite build risk.

Evidence:

- `df -h / /tmp /nix /var/lib/factory` reported `rootfs` at `24G used, 0 available, 100%` for `/`, `/tmp`, and `/nix`.
- `/var/lib/factory` has space (`69G` available), but Nix store writes use the full root filesystem.
- Zed build and scanner retries failed with `No space left on device`.
- Removing validation-created `/tmp/opencode/zed-project`, `/tmp/opencode/klocc-waybap.OFJT98`, and `/tmp/opencode/perf.data` freed only about 108 MiB; `/` remains effectively full.
- Later cleanup restored about `24G` free on `/`, `/tmp`, and `/nix`, enough for normal local checks and package-sized scanner validation but not enough to justify another full Zed prerequisite build attempt.

Open questions:

- Can we safely run Nix garbage collection on this machine, or should the operator free root/Nix store space manually?
- Should validation scripts place all clone/build/cache outputs under `/var/lib/factory` and avoid `/tmp`/root-backed paths?

Immediate follow-up:

- Run heavy Nix/Cargo validation one target at a time to avoid eval-cache contention and another sudden Nix store fill-up.
- Treat very large third-party prerequisite builds as cache/space-gated rather than scanner validation until their build outputs are already realized.
