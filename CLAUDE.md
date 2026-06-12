# Project Instructions

## GPUI Native UI Work

Before implementing or changing any GPUI UI code, first inspect the current GPUI source and examples instead of relying on memory or guessed APIs.

- Clone or update the Zed/GPUI source outside this repository, for example under `/tmp/opencode/zed-gpui`.
- Read the relevant GPUI examples and docs for the UI primitive being used before editing code.
- For app/window/view structure, inspect `crates/gpui/README.md`, `crates/gpui/docs/contexts.md`, and `crates/gpui/examples/hello_world.rs`.
- For custom treemap rendering, inspect `crates/gpui/examples/painting.rs` and use `canvas(...)`/window paint primitives rather than building thousands of DOM-like child elements.
- For system file selection, inspect `App::prompt_for_paths` and `PathPromptOptions` in GPUI source.
- In final summaries for GPUI work, mention which GPUI source files/examples were checked.

The first native UI is a SQLite artifact viewer only. It must not run scans itself unless the product direction changes explicitly.
