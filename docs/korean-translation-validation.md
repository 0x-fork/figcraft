# Korean translation validation

Catalog additions against `d9c924c3ba7114dff9141c667aec56d3764f3af9`: **141** entries. Existing Korean entries are unchanged. The provided translation data was checked against the current Rust sources; candidate keys with no matching production literal were excluded. No new translations were generated.

## Static checks

- Catalog structure, unique keys, original key preservation, named placeholders, trailing ellipses, and explicit shortcut tokens checked with Python 3.14.5.
- FigCraft TOML was parsed with the standard TOML parser; Korean plural entries keep only `other`.
- New entries have source-location evidence in the local audit; dynamic labels use the existing translation paths.
- `git diff --check` passed.

## Checks not run

Cargo, rustc, and rustfmt are unavailable in this environment. Rust installation is awaiting user approval; no tools were installed. The following checks have **not** been run:

- `cargo test -p figcraft-ui-egui`
- `cargo fmt --all --check`
- `cargo clippy -p figcraft-ui-egui --all-targets -- -D warnings`
- `cargo xtask layers`, `cargo xtask assets`, `cargo xtask wasm`, and the full `cargo xtask ci` gate
- Native and web UI rendering, layout, runtime language switching, and font/glyph coverage

## Candidate filtering

- Excluded 0 supplied candidate keys without a matching production source literal.
- Excluded 0 supplied keys already proposed in open Korean translation PRs.
