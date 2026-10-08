# lexical-js-shim

Runs **Lexical for JavaScript** plugins inside lexical-rs. The shim is a WebAssembly
component made of a JavaScript engine (StarlingMonkey, via `jco componentize`), the real
`lexical` packages, and about 150 lines of glue (`src/shim.js`). It implements the
`document-plugin` interface in `crates/lexical-plugin/wit/lexical.wit`.

```
host (Rust)                                   component (JavaScript)
 command, document as Lexical JSON, selection ─►  headless Lexical editor
                                                  ├─ registerRichText
                                                  └─ registerMarkdownShortcuts …
 one ordinary update with the new document   ◄──  document as Lexical JSON, selection
```

Which Lexical plugins run is `src/plugins.js`; register anything that needs an editor and
no DOM, exactly as you would in a web page.

```sh
npm install
npm run build        # dist/lexical-js-shim.wasm (about 20 MB)
```

```rust
let plugin = lexical_plugin_host::WasmDocumentPlugin::load(&std::fs::read("dist/lexical-js-shim.wasm")?)?;
editor.add_plugin(Box::new(plugin));
```

## What this showed

* Unmodified `@lexical/markdown` shortcuts and `@lexical/rich-text` run in the component:
  typing `## ` makes a heading, `- ` a list, and plain typing, Enter and Backspace give the
  same documents as the native engine. Five keystrokes took about 40 ms in a release build.
* Our node tree is Lexical-JSON compatible, so the document crosses the boundary as JSON
  with no translation. The selection crosses as `{block, offset}`, where a block is one that
  holds inline content, counted in document order on both sides.

## Accommodations

* The JS runtime has no `process`, so `build.mjs` fixes Lexical's `NODE_ENV` switch.
* Its regex engine rejects Unicode property escapes (`\p{L}`), which occur only in
  `@lexical/link`'s auto-link matcher; `build.mjs` swaps in close approximations.
* Lexical commits in a microtask, so the shim flushes with discrete updates before reading
  the result; that is when update listeners such as markdown shortcuts run.

## Limits

* **Stateless**: each call builds a fresh editor, so a JS plugin cannot keep state between
  commands and node keys are not stable across calls.
* **Cost**: every command is a whole-document round trip. Use the fine-grained `plugin`
  interface for anything on a hot path.
* **Node types**: documents are limited to what lexical-core can represent (paragraph,
  heading, quote, code, lists, links, text, line breaks). Tables, hashtags, marks and custom
  or decorator nodes would not survive the return trip.
* **No DOM**: plugins that need a root element, mutation listeners or decorators cannot run.
* **Commands**: five are named (`insert-text`, `insert-paragraph`, `delete-backward`,
  `delete-forward`, `format-text`); everything else, including undo, stays with the editor.
* **Size and build time**: a 20 MB component, and compiling it takes minutes in a debug
  build, so the tests run in release mode.
