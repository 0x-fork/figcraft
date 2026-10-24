# egui 0.37 local implementation — stage 1 notes and follow-ups

## Current local qualification — 2026-10-24

The local migration includes the reviewed source content of captured main
`3476f4b1e72e31514670ceff9c1a5a3d78a8b302`. The migration base is
`8167f13a6a416ca61fcb505a7eaf6be383f6c5c0`.
This local integration records both the migration base and captured main as parents.
Canonical immutable egui/winit/shared-library pins and the Cargo lock are resolved.
The normal UI dependency enables both `default_fonts` and `monochrome_emoji_fonts`.

The current source passed the repository's complete local `xtask ci`: formatting,
strict workspace/all-target lint, workspace tests and doctests, translation report,
layers, assets, five-package WASM check and plugin workspace fmt/lint/tests/WASM
build. The recorded 70 summaries contain 3,189 passing executions, zero failures,
nine ignored tests and 3,002 filtered instances. This includes a repeated translation
report test; it is not a count of distinct regressions. The main doctest stage selected
zero tests. All 17 field-save regressions passed, including owner-bound deferred saves,
same-pass document replacement and later valid edits/saves. The three real plugin
bundles and all six original outputs were hash-verified and preserved.

Incoming IME interruption and current-geometry screenshot admission repairs are
implemented, as are the original-document/window binding and per-field stale-draft
cancellation needed for deferred Save/Save As across tab/control routing. Historical
stage notes below describe the original implementation state rather than current
qualification. Local evidence retains the exact source/lock, focused tests, full CI
and original plugin outputs. Native/visible GUI and browser workflows, platform IME,
no-font configurations where relevant, paired performance and actual release package
installation/signing/AppImage inspection remain separate open gates. Further notice
repairs, if required, are not claimed complete here. No application publication has
been authorized; PRs require human review and human merge.

The following stage 1 notes are retained as historical implementation context.

## What this stage changes

- Dependency family at 0.37 (`egui`, `egui_extras`, `eframe`, the app's `clipboard` feature), with
  `ui-egui` enabling the production `default_fonts` fallback and tests explicitly enabling
  `default_fonts` + `monochrome_emoji_fonts` (kittest's own dev-features do not propagate to us).
- Mechanical API ports: `Id::new` → `Id::unique`, `Ui::id` → `Ui::scope_id`,
  `WidgetType` → `Role`, `MouseWheel::source`, `Shape::Band`, `ScrollArea::clamped_offset`.
- Lifecycle split: `FigApp::background_tick` (logic-safe work: control queues, live polling,
  background completions, autosave/recovery deadlines) and `FigApp::ui_setup` (fonts/language/
  theme/UI zoom/render worker, once per real frame). Direct `run_ui` harnesses tick the
  background once in `FigApp::ui` and clear the marker afterwards, so every direct frame ticks
  instead of every other one; `App::logic` ticks it for native/web. Child viewports do not repeat
  the root's background work, but each runs `FigApp::viewport_setup` in its own frame: theme and
  `ui.dark`, language, fonts (including CJK faces for its own document) and the zoom plan.
- A monotonic `FrameClock` (`web_time::Instant`) drives autosave, auto-save and capture deadlines.
  Tests inject it (`set_test_now`); UI-time (`ctx.input(i.time)`) is not used for deadlines.
- Local capture-slot adapter in `crates/ui-egui/src/control.rs` (no `craft-ui` dependency):
  weak one-shot callback, per-request identity/serial, deadline/cancel, document + viewport
  generation, capture-time geometry epoch (size/DPI/zoom only, not window position), retry on
  geometry changes, fail-closed completion when current geometry is unrepresentable, error on
  document changes, straight-RGBA PNG reply, and the 4-request / 128 MiB-pixels /
  256 MiB-aggregate budget counted over in-flight and completed-but-undrained images. The budget
  reserves the larger of the encoder phase (readback + straight-RGBA copy + PNG) and the reply
  phase (readback + PNG + base64); the encoder copy is dropped before base64 is built, and a
  post-encode reply cap refuses an oversized PNG before the base64 string is allocated. The
  request's document and window are fixed at admission and never rewritten at issuance, so a tab
  switch while settling is an error rather than the new tab's pixels. Admission and issuance happen
  in the UI phase. A hidden request, or one whose platform visibility is not reported, is parked
  with its deadline and asks for `RequestPaintWhileHidden`; a completed slot asks again for the
  real pass that drains it instead of waiting for the deadline. Settlement counts completed UI
  frames, so a discarded sizing pass never serves as a settled capture frame.
- Accessibility: the custom panel edges (`Id::unique(("panel_edge", …))`) no longer sense focus;
  `a11y::begin` records the root `Ui::scope_id` so a real `panel.__resize` ID is derivable if a panel
  ever becomes resizable. Preferences (`prefs.panel_widths`) and fixed geometry are unchanged.
- Synthetic input from the control channel: one pending batch per window, tagged with the
  document it was injected for (dropped if the active file changed), queued in the live pane of the
  window drawing now (a child window's batch is not written into the swapped-out default pane),
  applied exactly once per raw-input frame (shortcuts return on a discarded pass's rerun instead of
  inserting or running again), acknowledged only from the pass that presents it, and fenced from the
  user's system clipboard in one place: `FigApp::clipboard_set_text`/`clipboard_set_png` drop every
  copy path's write (copy/cut, copy-as-CSS/SVG/PNG/text/link, property/code/comment/join-code
  copies) while the frame's input is synthetic. A synthetic `Cut` still deletes the text-edit
  selection in the document; only its OS clipboard write is fenced. Acknowledgements belong to the
  window whose batch they acknowledge, so an empty child frame cannot error the root's. Focus-epoch fencing for
  text/IME batches remains part of the E21b nextstep.
- Incoming IME byte deletion is implemented for the custom text editor
  (`figcraft_engine::textedit::delete_surrounding_bytes`, wired from `ImeEvent::DeleteSurroundingBytes`)
  and tested for multibyte CJK, a supplementary emoji, deletion-only input and one undo step.
  Partial characters are preserved (a byte count ending inside one deletes only the whole
  characters it covers, matching the fork and egui's `surrounding_bytes_to_chars`). The selection
  itself is kept, as in stock `TextEdit`, so the paired `Commit` replaces it; only the complete
  characters before/after the selection go. A delete that finds nothing is a true no-op: it keeps
  an in-progress composition (pre-edit), the revision and the document untouched. The older
  character-counted `DeleteSurrounding` event follows the same rules and has an app-level test.

## Outgoing IME surrounding text (E21b/P7b)

Implemented for FigCraft's one custom IME producer, `shortcuts::ime_output`, against the reviewed
fork API. The descriptor now carries:

- a stable owner `(document uid, text node)` and the engine's `TextEdit::editing_session` as the
  replacement epoch (a document switch or another node changes the owner; re-entering the same
  node starts a new generation without reusing the old one);
- the tracker's viewport-scoped `context_generation`, observed exactly once per viewport pass
  after the pass's IME events and engine commands. `FigApp::observe_ime` is the single call site
  (editor passes, the home/present early returns and the first-font frame all pass through it);
  an idle `observe(None)` followed by a focused one in the same pass would drop receipts and bump
  the generation, so the early shortcut/palette idles were removed. A late `window.new` that moves
  the edited file to another window makes the old viewport observe idle for that pass;
- a bounded (≤ 4000 UTF-8 bytes) committed-text snapshot from
  `figcraft_engine::textedit::ime_surrounding`: logical model text, never `bidi::display`, with the
  UI-only pre-edit excluded, character caret/anchor converted independently to UTF-8 byte offsets
  (reversed selections stay reversed), and an explicit `Unavailable` reason when no edit/text node
  exists or the selection alone exceeds the budget. Empty text is `Available("",0,0)`.
- explicit change cause: `InputMethod` only for an accepted `Commit`/`DeleteSurrounding*` receipt
  captured immediately around that mutation (delete+commit chains are one receipt chain); plain
  typing, paste, keys, text shortcuts, undo, control-channel and AutoCorrect/list AutoFormat
  changes report `Other` and dominate the frame. No-op, failed and stale receipts earn nothing.
  When the bounded snapshot is unavailable (for example a selection wider than 4000 bytes), both
  the pass observation baseline and the pre/post receipt stamps carry the engine's document
  revision, so an accepted commit that replaces that selection still earns `InputMethod`.
- `geometry` remains the actual caret (the composition cursor while composing).

A hidden/unfocused/no-editor pass observes `None`, and closing a child window calls
`end_viewport` (now covered by a windows regression). The outgoing text is copied only as the
bounded window; the query never clones the document. Converting the caret/anchor character
indices to UTF-8 bytes now takes one character walk (both offsets) instead of two or three full
scans. Remaining cost: the caret is a character index, so a caret near the end of a huge layer is
still O(n) per query; `ime_stamp` runs the query before and after each text-affecting event, so
such a layer pays O(caret/anchor) per event and at most 4000 copied bytes. The generic edit path
also still counts characters once to clamp the caret and clones the pre-edit document for
normalisation/undo.

Tests: engine (`ime_surrounding` Unicode/reversed selection, pre-edit exclusion, empty/bounded/
too-wide selection, stable owner and session replacement, reverse-selection byte deletion with a
preserved selection, character-counted deletion, no-op composition preservation, list AutoFormat
and normalisation classification, double-click epoch stability) and UI (`ime_tests.rs`: commit vs
typing/mixed causes, transformed commit cause, bare-newline/control no-ops, byte and character
delete+commit, pre-edit, logical UTF-8, empty vs unavailable, owner replacement, double-click
inside the edit, one observation per frame, late `window.new`, discarded pass, two viewports).
These are source tests only; the parent runs the managed build/test commands.

## Web hidden paint

`RequestPaintWhileHidden` is honoured by native eframe but not by the shared web runner, which logs
an unhandled command for hidden tabs. Hidden-browser-tab `ui.screenshot`/`ui.input` therefore remain
unqualified even though the native path now queues and settles them. This is a forkfix prerequisite,
not a passing claim.

## Local dependency and lock prerequisites (parent)

- Parent config `local-integration-config.toml` patches the egui family and winit, and `craft-ui`/
  `craft-print`. It does **not** patch `craft-window`. FigCraft now disables craft-window's optional
  `egui` feature (it only needs the frame probe), but Cargo still resolves a package's optional
  dependencies into the lock, and that rev declares `egui = "0.36"`. The managed graph must
  therefore also patch/repoint `craft-window` to the reviewed craft-libs revision whose workspace
  targets 0.37 (or otherwise prove the lock has exactly one egui family); otherwise the lock will
  contain a second egui 0.36.
- No `[patch]` rows or path dependencies live in this repository's manifests; the parent generates
  `Cargo.lock`. Do not publish this tree's resolved paths.
- Fonts: production keeps its central `CRAFT_FONTS_DIR`/system-font policy; no font binaries are
  added here. Color-emoji claims still require a separate `epaint/color_fonts` feature job.

## Qualification gaps and baseline backports/perf operations

Synthetic `ui.input` for deferred child viewports still relies on the existing per-pane queue and
`windows::take_input`; eframe invokes `App::raw_input_hook` for the root update, so injected input
into a child window is not newly qualified here.

Surface generation is not exposed by the pinned eframe/egui API, so the capture epoch covers
logical content size, DPI and zoom but cannot distinguish a same-size surface loss/recreation; that
remains a qualification gap (reported by the sibling capture review).

Remaining lifecycle item from the plan, not changed in this stage: prototype/after-delay/animated-GIF
clocks still advance from `present::show` and the GIF drawing path (UI only). They should move to a
non-drawing step called from `background_tick` so hidden playback advances once per deadline and the
visible pass never repeats it.

Unverified here: compile/tests/clippy/fmt, native X11/Wayland/Windows/macOS, browser WebGPU/WebGL2,
screen readers, hidden-window forced paint, PRIMARY/W15P, and all performance numbers. Baseline
operations to measure once the parent can build: `#8731` text tessellation, `#8736` DX12 pipeline
startup, `#8512` font discovery, `#8763`/PR `#8764` TextEdit click-away, `#8748` hidden deferred
children. None of these were applied, removed or claimed as fixed.

## Optional Latin manifest compatibility

The pinned central font catalog has a required `fonts/manifest.txt` and no optional `fonts/latin-manifest.txt`. Required-font builds now validate the core and all supplied extension assets, while an absent opt-in Latin manifest is watched but not required. This follows the existing build script documentation and keeps malformed or incomplete supplied manifests fatal in required mode. No font binary was copied into FigCraft. Both required-core and no-font configurations remain validation gates.
