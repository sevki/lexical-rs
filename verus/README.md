# Formal verification with Verus

Modelled on [dafny-replay](https://github.com/metareflection/dafny-replay): **verified kernels,
proved once, plugged into a domain that owes only a small set of obligations.**

```text
Domain (Doc, Cmd, Inv, Init, Apply, Normalize)      domains/editor.rs     <- obligations R1, R2
        |  plugged into
Replay kernel     History = {past, present, future}  kernels/replay.rs     <- undo/redo for ANY domain
Authority kernel  Server  = {version, present, log}  kernels/authority.rs  <- sync for ANY domain
        |  refined by / bridged to production
production_history.rs   abs(History in lexical-core) = kernel History
bridge/                 reconciler diff, selection offset mapping, split_text remap
theorems.rs             the kernels instantiated with the editor
```

**Read [`GUARANTEES.md`](GUARANTEES.md)** for exactly what is proved, what is obligated and what is
trusted. In one line: *if every editor command preserves the document invariant, then every state
reachable through typing, undo, redo, time travel, stale or hostile clients, and optimistic
re-basing also satisfies it — by construction* — plus per-command laws such as "Enter in a paragraph never loses a
character" and "Backspace at a block start merges without losing text".

## Running

```sh
VERUS=/path/to/verus verus/verify.sh      # or put `verus` on PATH
```

CI downloads the latest Verus release and runs this on every pull request. Verus is a fast-moving
project (the mutable-reference semantics changed recently); if CI breaks after a Verus release,
this directory was last verified with Verus commit `2c9bf54` (rolling 2026-10-07).

Building Verus from source (what was done to develop these proofs when release downloads were
unavailable): `rustup toolchain install` per `rust-toolchain.toml`, `pip install z3-solver==4.16.0.0`
and symlink `z3` into `source/`, then `source ../tools/activate && vargo build --release`.

## Honest limits

Verus verifies a subset of Rust, so these are models mirrored from the production code, not the
production code itself — see "Integration boundary" in `GUARANTEES.md`. The runtime counterpart
`EditorState::check_invariants`, driven by random traces in
`crates/lexical-core/tests/invariants.rs`, covers the tree-level invariants the models abstract away.
