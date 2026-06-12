# GUI Rendering Diagnostics Plan

Date: May 26, 2026

## Goal

Make `klocc-gui` rendering correctness and performance easier to debug by separating structured layout facts, visual screenshots, and performance telemetry. The first implementation should help find issues; it should not lock the project into a heavy fixture or golden-file system before the useful signals are proven.

## Current Findings

### Existing Methodologies

| Methodology | Existing path | Current role | Notes |
| --- | --- | --- | --- |
| `unit` | `nix/unit/default.nix`, Rust `#[cfg(test)]` tests | CLI/scanner and layout/property-style tests via Cargo | `crates/klocc-gui/src/layout.rs` already contains strong generated layout invariants. |
| property-style Rust tests | Rust tests co-located in `layout.rs`, currently run by `unit` checks | Generated treemap geometry cases | Methodologically property testing, but not yet an adopted `nix/property/` methodology. Do not advertise it as adopted until it has a wrapper. |
| `snapshot` | `nix/snapshot/default.nix`, scenarios/environments/assertions | Hyprland/Weston GUI screenshot checks | Already emits rich layout text into `klocc-gui.log`, but assertions mostly inspect aggregate telemetry and pixels. |
| `pre-observability` | `nix/pre-observability/default.nix`, `gui-wayland-perf-check.sh` | App-internal perf run with log/metric queries | Useful for budgets, but separated from screenshot scenarios. |
| `side-by-side` | `nix/side-by-side/default.nix` | Real scanner artifact checks across targets | Not directly about rendering, but produces realistic artifacts. |

### Existing Rendering Signals

`crates/klocc-gui/src/main.rs` already logs these per uncached layout/paint pass:

| Signal | Source | Current use |
| --- | --- | --- |
| `klocc-gui: layout ... rects in ...ms` | `treemap_canvas` layout closure | Parsed by snapshot telemetry assertions. |
| `klocc-gui: display summary ...` | `layout_debug_summary` | Human-readable only. |
| `klocc-gui: display size histogram ...` | `layout_size_histogram` | Human-readable only. |
| `klocc-gui: display ascii map ...` | `layout_ascii_map` | Human-readable only. |
| `klocc-gui: display rects ...` | `layout_rect_manifest` | Human-readable only; contains structured-enough rect lines. |
| `klocc-gui: paint quality ...` | paint closure | Parsed by snapshot and perf checks. |
| `klocc-gui: paint profile ...` | `KLOCC_GUI_PROFILE=1` | Parsed by pre-observability perf check. |
| `klocc-gui: response ...` | interaction timing | Parsed by pre-observability perf check. |

### GPUI Constraints Checked

Source inspected under `/tmp/opencode/zed-gpui`:

| File | Relevant fact |
| --- | --- |
| `crates/gpui/README.md` | GPUI is hybrid immediate/retained; custom rendering belongs in low-level elements/canvas where needed. |
| `crates/gpui/docs/contexts.md` | `Window`, `Context<T>`, and `App` responsibilities match current `Viewer` structure. |
| `crates/gpui/examples/painting.rs` | Custom painting via `canvas(...)` and `window.paint_quad(...)` matches the current treemap implementation pattern. |
| `crates/gpui/src/elements/canvas.rs` | Confirms `canvas` split between request-layout and paint closures. |
| `crates/gpui/src/window.rs` | Confirms `on_next_frame` and `paint_quad` APIs used by current code. |

## Diagnosis

The current snapshot checks are visually useful, but they make debugging slower because the data is trapped in a long log stream. When a click hits the wrong source, when a metric/filter state produces too few rects, or when a layout regresses into slivers, the next person has to reconstruct facts from screenshots plus `klocc-gui.log`.

The highest leverage first step is not a new renderer, fixture corpus, or GPUI test harness. It is extracting the existing layout manifests into per-step artifacts and asserting scenario-specific facts from them, while being explicit about whether a step produced a new layout or intentionally reused the prior one.

## Proposed Layering

| Layer | Methodology | Artifact | What it catches |
| --- | --- | --- | --- |
| Layout facts | `snapshot` initially, later maybe `property`/`unit` fixture checks | per-step `*.rects` extracted from `klocc-gui.log` | wrong root, wrong metric/filter state, wrong target source, duplicate label ambiguity, slivers/tiny buckets, bad click hit. |
| Visual pixels | `snapshot` | per-step `*.png`, `walkthrough.png` | compositor/capture/font/color/visual regressions. |
| Performance budgets | `pre-observability` | `perf-summary.tsv`, app log | slow layout/paint/response paths. |
| Algorithm invariants | `unit` today, future `property` only if `nix/property/default.nix` is adopted | Rust property-style tests in `layout.rs` | overlap, area ratio, local grouping, tiny merge correctness. |
| Scenario replay | future `replay` | captured semantic scenario actions | regression locking for hard-won interaction bugs. If adopted, it belongs under `nix/replay/default.nix`; snapshot may share helpers through `nix/shared/`, not absorb replay as an unbounded subfeature. |

## Minimal Useful Implementation

Implement only the first diagnostic slice:

1. Add per-step rect-manifest artifacts to snapshot outputs by extracting layout blocks from `klocc-gui.log` using a step-bounded log line cursor in both snapshot environments.
2. For each step, write metadata into the top of the `*.rects` file: scenario id, step id, status, previous log cursor, current log cursor, and source line range when a new block is found.
3. If a step produces no new manifest block, write `status=reused-unknown` plus `reused_from=<previous-step-stem>` when a prior manifest exists. Do not assert metric/filter/root facts from reused-unknown evidence.
4. Add optional scenario assertions under `assertions.layout` that inspect per-step manifests.
5. Add only low-risk assertions for current scenarios:
   - `treemap-drilldown`: every step must have a non-empty rect artifact with metadata and either a `rect manifest begin` block or an explicit `reused_from` marker.
   - `metric-runtime-drilldown`: every step must have a non-empty rect artifact; semantic xgcc/root assertions are deferred until the app logs cache-hit state or emits JSON manifests.
6. Preserve existing screenshots and telemetry checks unchanged.

This improves issue-finding because each snapshot result directory will contain direct files like `01-total-reach.rects`, not just a monolithic `klocc-gui.log`.

## Layout Ownership Rule

The minimal slice must not add a new direct child under `nix/`. Put extraction/assertion helpers under `nix/snapshot/` or generic helpers used by multiple methodologies under `nix/shared/`. A new direct child like `nix/replay/default.nix` or `nix/property/default.nix` requires explicitly adopting that methodology and wiring it through `nix/default.nix`.

## Deliberately Deferred

| Deferred item | Reason |
| --- | --- |
| JSON render manifest emitted by Rust | Useful, but it changes app code and schema before proving the harness-level extraction is enough. Revisit if text parsing becomes brittle or stale-state detection remains weak. |
| Checked-in SQLite fixture corpus | Valuable, but fixture generation/storage needs a separate decision. |
| GPUI `#[gpui::test]` harness | Promising for unit-level UI state, but not needed to improve snapshot debugging now. |
| Scenario-specific semantic click targeting | Bigger product/test DSL change; first expose current facts. |
| Per-step timing extraction | Keep performance gates in `pre-observability`; defer timing artifacts until rect extraction proves useful. |

## Risks

| Risk | Mitigation |
| --- | --- |
| Text manifest parsing becomes brittle | Start with block extraction and simple line checks; avoid complicated regex schemas. If this grows, promote to Rust-emitted JSON. |
| Per-step manifests are stale | Track step offsets/block counts and mark each artifact `new` or `reused`; fail state-changing expectations that do not produce expected new state evidence. |
| More artifacts make review noisier | Add only `.rects` files, grouped by existing step stem. |
| Assertions duplicate Rust layout tests | Snapshot layout assertions should focus on scenario state/hit outcomes, not algorithm invariants already covered in Rust. |
| Weston/Hyprland diverge | Extract manifests from app logs in both environments rather than compositor-specific channels. |
| Placeholder files mask missing diagnostics | Rect artifacts must be non-empty and contain metadata; placeholder creation must not satisfy diagnostic assertions. |

## Success Criteria

| Criterion | Verification |
| --- | --- |
| Each snapshot step produces a non-empty `*.rects` artifact | Build one Weston scenario and inspect result directory. |
| Rect artifacts are listed by scenario artifact policy | Existing artifact copy logic includes them. |
| Existing snapshot checks still pass | Build at least `gui-snapshot-treemap-drilldown-weston`. |
| Failure messages point at layout facts | Add assertion errors that name scenario step and missing pattern. |

## Iteration Log

### Draft 1

Plan favors harness-level rect extraction over immediate Rust JSON export. This is intentionally minimal and reversible.

### Draft 2

Accepted architect review pass 1 corrections:

| Correction | Applied change |
| --- | --- |
| Latest manifest can be stale | Require step-bounded extraction and explicit `new`/`reused` metadata. |
| Generic source-rect assertions are too weak | Add scenario-owned semantic expectations for state transitions and target labels. |
| Hardcoded root ids are brittle | Prefer semantic label/metric/filter evidence over numeric source ids unless using a fixed fixture. |
| `property` was incorrectly advertised as adopted | Reworded as property-style tests currently run by `unit`. |
| Minimal slice does not gate performance | Narrowed claim and added diagnostic `*.timing` files only. |
| Future replay could bloat snapshot | Bounded replay as a future `nix/replay/` methodology. |

### Draft 3

Accepted architect review pass 2 corrections:

| Correction | Applied change |
| --- | --- |
| Step-to-manifest binding still underspecified | Require both snapshot environments to capture a `klocc-gui.log` line cursor per step and extract only newly added blocks. |
| Artifact copy can hide missing diagnostics | Require non-empty rect artifacts with metadata; placeholders do not satisfy assertions. |
| Reused state is unverifiable from current logs | Use `status=reused-unknown` and defer semantic assertions from reused evidence. |
| `.timing` is scope creep | Removed timing artifacts from minimal implementation. |
| Property still looked adopted | Clarified algorithm invariants are `unit` today, future `property` only after adopting `nix/property`. |
| Need no-new-direct-`nix/` rule | Added layout ownership rule. |
