//! Rust types generated from the plugin contract in `lexical-plugin/wit`.

wasmtime::component::bindgen!({
    path: "../lexical-plugin/wit",
    world: "lexical-plugin",
});
