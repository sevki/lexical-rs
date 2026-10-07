# Formal verification with Verus

[Verus](https://github.com/verus-lang/verus) checks Rust code against specifications with an SMT
solver. Verus only accepts a subset of Rust (no `HashMap`, trait objects, GTK…), so the production
crates are not verified directly. Instead each module here is an **executable model that mirrors one
function of the production code**, and is proven correct for *all* inputs (not just tested ones).

| Module | Mirrors | Properties proven |
|---|---|---|
| `history.rs` | `History` / `Editor::{undo,redo}` in `lexical-core` | `undo.len + redo.len <= limit` is invariant; a commit always clears redo; `undo` restores exactly the saved state; **undo∘redo and redo∘undo are the identity** on the whole editor; editing after undo discards redo |
| `diff.rs` | `reconciler::apply` text diff in `lexical-adw` | the single splice (`old[0..p] + new[p..new_end] + old[old_end..]`) **equals `new` exactly**, so the widget text always equals the layout text; edit bounds are valid; no-op on equal strings |
| `layout_sync.rs` | `Layout::{offset_of, point_at}` | line starts are strictly increasing; offsets never leak into the next line; **`point_at(offset_of(p)) == p` for every valid point** (lossless model↔widget selection sync); offsets inside list markers clamp to content start |
| `split.rs` | selection remap in `EditorState::split_text` | an offset lands in exactly one piece at the same character; first-fit on shared boundaries; piece lengths sum to the node length |

## Running

```sh
VERUS=/path/to/verus verus/verify.sh      # or just put `verus` on PATH
```

CI downloads the latest Verus release and runs this on every pull request.

## What this does *not* prove

* The models are hand-mirrored. They can drift from the Rust they describe; when you change
  `history.rs`, `reconciler.rs`'s diff, `Layout::point_at` or `split_text`, update the matching model.
  (Differential tests in `crates/lexical-core/tests` exercise the same behaviours on the real code.)
* Not covered: the tree-editing algorithms (`delete_between`, list indent/outdent), Unicode
  grapheme segmentation, GTK itself. These are tested but not proven.
* "Sync" here means model↔view synchronisation (selection mapping and buffer reconciliation).
  There is no multi-user/collaborative sync in the engine yet.
