# GUARANTEES

What is **proved** (Verus), what each domain **owes** (obligations), and what is
**trusted** (integration boundary). Same structure and vocabulary as
[dafny-replay's GUARANTEES](https://github.com/metareflection/dafny-replay/blob/main/GUARANTEES.md):
generic kernels are proved once; the Lexical editor is a *domain* that discharges the
obligations; everything outside the model is listed as trusted.

Verified with `verus/verify.sh`: **86 verified, 0 errors** (Verus rolling release, 2026-10-07).

## Notation

* `Model` = `Doc`, `Action` = `Cmd` (see `domains/editor.rs`)
* `Inv : Doc → bool` — the document invariant
* `Init()` — one empty paragraph, caret inside it
* `Step(m, a) = Normalize(Apply(m, a))`
* `History = { past, present, future }`, `Server = { version, present, log }`

## Replay kernel — `kernels/replay.rs`

### Domain obligations

* (R1) `Inv(Init())`
* (R2) `Inv(m) ⇒ Inv(Normalize(Apply(m, a)))`

### Kernel theorems (proved once, for every domain)

* (RK1) For **every trace** of `Do` / `Undo` / `Redo` operations, all states in the history
  (past, present, future) satisfy `Inv` — `trace_preserves_inv`
* (RK2) `Do`, `Preview`, `Merge`, `Undo`, `Redo` each preserve the history invariant
* (RK3) `Do` leaves no redo branch
* (RK4) `Undo(Do(h, a))` restores `h.present` and `h.past`; `Redo(Undo(h)) = h` and
  `Undo(Redo(h)) = h` whenever the operation acts
* (RK5) The executable `ReplayExec` computes exactly the specified transitions; the bounded
  variant keeps `past.len() ≤ limit` and every retained step satisfies `Inv`

### Refinement to production — `production_history.rs`

`History` / `Editor::{undo, redo, commit}` in `lexical-core` is modelled as `Prod`, with
`abs(p) = { past: p.undo, present: p.cur, future: reverse(p.redo) }`. Proved: `undo`, `redo` and
both `commit` modes (merge / push, bounded) map to the corresponding kernel operation under
`abs`, and `undo.len + redo.len ≤ limit` is invariant. So RK1–RK4 hold for the production
history *as modelled*.

## Authority kernel (sync) — `kernels/authority.rs`

### Domain obligations

* (A1) `Inv(Init())`, (A2) `Inv(m) ⇒ Inv(Step(m, a))` (same as R1/R2); `accepts` is an
  arbitrary validity gate.

### Kernel theorems

* (AK1) For any sequence of requests — any client, stale versions, arbitrary actions — the
  server state satisfies `Inv` — `serve_preserves`
* (AK2) Rejected requests (stale or domain-invalid) leave the server unchanged
* (AK3) `version = |log|`; it grows by exactly one per accepted request and otherwise never changes
* (AK4) **Replay:** the server state is always `fold(Step, Init, log)`
* (AK5) A non-stale request the domain accepts is applied (no spurious rejection)
* (AK6) Optimistic clients: what the user sees (server base + pending local actions) always
  satisfies `Inv` after local edits, acknowledgements and re-basing onto a fresh server state

## Editor domain — `domains/editor.rs`, `theorems.rs`

Obligations R1 and R2 are discharged for all ten commands (insert, enter, backspace, forward
delete, set block kind, indent, outdent, format, select, select-all). `Inv`:

* at least one block; every block kind in range (heading 1–6, list type ≤ 2, depth ≤ 8) and
  indent ≤ 10
* anchor and focus point inside the document (block exists, offset ≤ block length) —
  `Normalize` is the model of `EditorState::validate_selection`, run on every commit

Intent properties ("delta laws"), proved per command:

| Command | Law |
|---|---|
| Insert | adds exactly the inserted characters (plus replaces the selection) |
| Delete selection | removes exactly the characters between the endpoints (`delete_range_conserves`) |
| Backspace inside a block | removes exactly one character (`backspace_removes_one`) |
| Forward Delete | preserves `Inv` (R2); the exact one-character law is proved for Backspace only |
| Backspace at block start | merging into the previous block loses **no** characters (`backspace_merge_loses_nothing`) |
| Enter | conserves every character, adds exactly one block (`enter_conserves_text`) |
| Kind / Indent / Outdent / Format | conserve the block count and every block's length; formatting leaves code points untouched |
| Select | never edits content |

Corollaries: `editor_history_is_always_valid`, `editor_server_is_always_valid`,
`undo_restores_previous_document`, `optimistic_view_is_valid`.

## View sync bridge — `bridge/`

These shrink the "trusted UI wiring" boundary that other verified-kernel projects leave open.

* `diff.rs` (`reconciler::apply`): the single splice the reconciler applies to the `GtkTextBuffer`
  turns the old text into **exactly** the layout text; the edit is in bounds and a no-op iff equal
* `layout_sync.rs` (`Layout::{offset_of, point_at}`): `point_at(offset_of(p)) = p` for every
  valid point; offsets inside a list marker clamp to the content start; no overflow
* `split.rs` (`EditorState::split_text`): selection offsets land in exactly one piece at the
  same character; piece lengths sum to the node length

## Integration boundary (trusted)

Not proved, even though the models are meant to mirror the code:

* **Model ↔ code correspondence.** Verus cannot check the production crates (`HashMap`, trait
  objects, `Rc<RefCell>`, GTK). The models are hand-mirrored; changing `history.rs`,
  `Layout`, the reconciler diff or `split_text` requires updating the matching model. This is
  the main trust gap, and the reason for the next item.
* **Runtime counterpart.** `EditorState::check_invariants` (tree shape, node-kind nesting,
  normalization, selection validity) is asserted after every step of thousands of random
  command/undo/redo/reload traces in `crates/lexical-core/tests/invariants.rs`. That is testing,
  not proof — but it found three real bugs while this was written.
* The tree algorithms themselves (inline-node splitting, list nesting, link wrapping),
  Unicode grapheme/word segmentation, `serde_json`, the JetStream wire format, GTK/libadwaita,
  input methods, the clipboard, and the Verus/Z3 toolchain.
* Liveness, ordering/delivery of messages, persistence, and authentication.
* Multi-client conflict resolution beyond reject-and-resync (dafny-replay's multi-collaboration
  kernel with rebasing and "intent envelopes") is **not** modelled yet.
