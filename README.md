# lexical-rs

A Rust port of the [Lexical](https://lexical.dev) rich text editor engine, modelled on
**Lexical iOS**, with its views built from GNOME primitives: GTK4 and
[libadwaita](https://docs.rs/libadwaita/latest/libadwaita/) via `gtk4-rs`.

| Crate | What it is |
|---|---|
| [`lexical-core`](crates/lexical-core) | Toolkit-agnostic engine: node tree, `EditorState`, range selection, update pipeline, node transforms, listeners, commands, history, Lexical JSON, flattened `Layout`. No GTK dependency. |
| [`lexical-adw`](crates/lexical-adw) | GTK4 / libadwaita views: `LexicalView` (editing surface), `Toolbar`, buffer reconciler, and a demo app. |
| [`lexical-sync`](crates/lexical-sync) | Real-time collaboration on [Loro](https://loro.dev): the document is flattened to one rich text (line-terminator characters carry block attributes, per-key marks carry inline formats), edits become minimal CRDT operations, remote updates are unflattened back into the editor. Local-only undo/redo, cursors that follow their text, and remote presence. |
| [`verus/`](verus) | Formal proofs (Verus) in the style of [dafny-replay](https://github.com/metareflection/dafny-replay): generic **replay** (undo/redo) and **authority** (sync) kernels proved once, the editor as a domain with proved invariants and per-command laws, plus proofs for the view-sync algorithms. See [`verus/GUARANTEES.md`](verus/GUARANTEES.md). |

```sh
cargo run -p lexical-adw --example demo     # needs libadwaita >= 1.5 (libadwaita-1-dev)
cargo test -p lexical-core                  # engine tests
cargo test -p lexical-core --features jetstream
xvfb-run -a dbus-run-session -- cargo test -p lexical-adw   # headless GTK tests
verus/verify.sh                             # proofs (needs Verus)
```

## How it maps to Lexical iOS

| Lexical iOS | lexical-rs |
|---|---|
| `Editor`, `editor.update { }` | `Editor::update(\|state\| ..)` — runs on a pending copy, **rolled back on `Err`**; then transforms → normalization → commit → listeners |
| `EditorState`, `Node`, `NodeKey` | `EditorState` arena (`HashMap<NodeKey, Node>`); `NodeData` enum: root, paragraph, heading, quote, code, list, listitem, link, text, linebreak |
| `RangeSelection`, `Point` | `Selection`, `Point` (`Text` / `Element` kinds), pending `format` for the next typed text |
| `registerUpdateListener` / `registerCommand` / `registerNodeTransform` | `register_update_listener` / `register_command(priority, ..)` / `register_node_transform` |
| `Plugin` | `Plugin` trait (+ `MarkdownShortcutsPlugin`: `# `, `> `, `- `, `1. `, `[ ] `, ```` ``` ```` ) |
| `HistoryPlugin` | built-in `History` with typing/deletion coalescing |
| `Reconciler` + `RangeCache` | `Layout` (flat text, line styles, runs, `offset_of` / `point_at`) + `lexical_adw::reconciler` (minimal text diff + `GtkTextTag`s) |
| `TextView` + `insertText`/`deleteBackward` overrides | `LexicalView`: a read-only `GtkTextView`; all edits come in through `IMMulticontext` commits, key bindings, toolbar and clipboard as `Command`s |
| JSON serialization | Lexical-compatible JSON (`to_json` / `from_json`), plus an optional binary form |

### Serialization

* **Lexical JSON** — `EditorState::to_json` / `from_json`, compatible with web Lexical documents
  (text format bits, `listType`, `tag`, `checked`, `indent`, alignment…).
* **[JetStream](https://jetstream.rs) wire format** (feature `jetstream`) — the document types
  themselves (`EditorState`, `Node`, `NodeData`, `Selection`, …) derive `JetStreamWireFormat`
  behind `cfg_attr`; there is no separate wire model. A document can be sent as an RPC message
  as it is, or as bytes with `to_wire_bytes` / `from_wire_bytes`. JetStream's own strings,
  vectors and maps count in `u16`, so three field codecs in `wire::wide` use `u32` counts for
  text, children and the node arena (a 260 KB text node and a 70,000-block document both
  round-trip). The arena is written in key order, so equal documents give equal bytes.
  Decoding checks nothing about how nodes relate, so use `from_wire_bytes`, or call
  `check_wire()` on a document received any other way, before trusting it.

## Status and known limitations

This is a redesign in Rust's idiom, **not a line-by-line translation** of the Swift code.

* Implemented: typing, paragraphs/line breaks, grapheme/word/line deletion, cross-block range
  deletion with block merging, inline formats (bold, italic, underline, strikethrough, code,
  sub/superscript, highlight), headings 1–6, quote, code block, bullet/numbered/check lists with
  nesting, links, alignment, indent, undo/redo, markdown block shortcuts, JSON.
* `EditorState` is cloned for each update (simple and gives free rollback/undo snapshots) — O(document)
  per update. Structural sharing (e.g. `im`) would be the next step for very large documents.
* The GTK view re-tags the whole buffer after each content change (text itself is diffed minimally).
* No IME pre-edit rendering yet (committed text works), no rich-text clipboard (plain text only),
  no node selection, no decorator/embedded nodes.
* Collaboration (`lexical-sync`): each local edit costs O(document) (flatten + diff); presence is
  encoded but transport is left to the app and the GTK view does not draw remote carets yet;
  text `mode`/`detail` and code `language` are not synchronised; a literal `\n` inside a text
  node becomes a soft line break. CRDT convergence itself is Loro's guarantee (trusted in the
  Verus model) and is covered by the tests below.
* Selection is not part of any serialized format.

## Testing

* `lexical-core`: 21 behaviour tests, 3 randomized trace tests, and 4 wire-format tests
  (`--features jetstream`). The trace tests run hundreds of random command / selection /
  undo / redo / reload sequences and assert `EditorState::check_invariants()` (tree shape,
  node nesting, normalization, selection validity) after every step; they found three real
  bugs while the proofs were being written. `LEXICAL_FUZZ_SEEDS=5000 cargo test --release
  -p lexical-core --test invariants` runs a deeper sweep.
* `lexical-sync` (the editor is first and foremost collaborative, so this is the largest
  suite): flatten/unflatten round-trips and totality against hostile marks; 19 concurrent-edit
  scenarios (same-spot inserts, overlapping deletes, bold vs. link, split vs. type, list
  restructuring, duplicate/out-of-order delivery, late joiners, anti-entropy); session
  behaviour (undo touches only your own edits, carets follow their text, read-only editors,
  presence); and randomised 2/3/5-peer sessions with delayed, reordered and duplicated
  delivery that check document invariants every step and identical documents at the end.
  `LEXICAL_FUZZ_SEEDS=500 cargo test --release -p lexical-sync --test fuzz` goes deeper.
* `lexical-adw`: a `harness = false` GTK test binary that drives the real input paths
  (IM commit, key handling, native selection, toolbar buttons) under Xvfb.
* Visual regression (PRs only): CI renders fixed editor scenarios (formats, headings, lists,
  selection + toolbar) offscreen for the **base branch and the PR head in the same job** and
  diffs them, so there are no committed baselines to go stale. Differences fail the job, with
  base / head / red-overlay diff images in the `visual` artifact; add the `visual-change` label
  to accept an intentional UI change. Run it locally with
  `xvfb-run -a dbus-run-session -- cargo run -p lexical-adw --example visual -- render out/`.
* `verus/`: 92 verified items — undo/redo and server-sync kernels, the editor domain's
  invariant and per-command laws, the production-history refinement, reconciler/selection sync.
* CI (`.github/workflows/ci.yml`) runs all of the above on every PR.
