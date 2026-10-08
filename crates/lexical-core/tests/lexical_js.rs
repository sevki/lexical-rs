//! Interop with Lexical for JavaScript.
//!
//! `fixtures/lexical-js.json` is real Lexical's own serialization (regenerate with
//! `node plugins/lexical-js-shim/fixtures.mjs generate`).

use lexical_core::EditorState;
use serde_json::Value;

const FIXTURE: &str = include_str!("fixtures/lexical-js.json");

/// Drop what the engine does not model so documents compare structurally.
fn essence(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut out = serde_json::Map::new();
            for (k, x) in m {
                if matches!(k.as_str(), "direction" | "version") {
                    continue;
                }
                out.insert(k.clone(), essence(x));
            }
            // `tab` and `code-highlight` are text subclasses; the engine keeps plain text.
            if matches!(m.get("type").and_then(Value::as_str), Some("tab" | "code-highlight")) {
                out.insert("type".into(), "text".into());
                out.remove("detail");
            }
            if out.get("type").and_then(Value::as_str) == Some("text") {
                out.remove("detail");
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(essence).collect()),
        x => x.clone(),
    }
}

#[test]
fn imports_lexical_js_document() {
    let s = EditorState::from_json_str(FIXTURE).expect("Lexical JS JSON must import");
    let text = s.to_plain_text();
    for part in ["hi", "inner", "x=1", "\t"] {
        assert!(text.contains(part), "missing {part:?} in {text:?}");
    }
}

#[test]
fn round_trips_lexical_js_document() {
    let original: Value = serde_json::from_str(FIXTURE).unwrap();
    let s = EditorState::from_json(&original).unwrap();
    assert_eq!(essence(&s.to_json()), essence(&original));
}

/// What Lexical for JavaScript must accept: `node plugins/lexical-js-shim/fixtures.mjs check`.
#[test]
fn exports_for_lexical_js() {
    let s = EditorState::from_json_str(FIXTURE).unwrap();
    let dir = std::env::var("LEXICAL_JS_EXPORT_DIR").ok();
    if let Some(dir) = dir {
        std::fs::write(format!("{dir}/rust-export.json"), s.to_json_string()).unwrap();
    }
}
