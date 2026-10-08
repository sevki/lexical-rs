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

#[test]
fn unmodelled_nodes_survive_a_round_trip_and_editing() {
    let doc = r#"{"root":{"type":"root","version":1,"format":"","indent":0,"direction":null,"children":[
      {"type":"paragraph","version":1,"format":"","indent":0,"direction":null,"children":[
        {"type":"text","version":1,"text":"a","format":0,"style":"","mode":"normal","detail":0},
        {"type":"mention","version":1,"name":"sevki","extra":{"n":[1,2]}}]},
      {"type":"table","version":1,"rows":[["x"]]},
      {"type":"paragraph","version":1,"format":"","indent":0,"direction":null,"children":[]}]}}"#;
    let original: Value = serde_json::from_str(doc).unwrap();
    let mut s = EditorState::from_json(&original).expect("unknown nodes must import");
    s.check_invariants().expect("invariants");
    let out = s.to_json();
    assert_eq!(out["root"]["children"][1], original["root"]["children"][1]);
    assert_eq!(out["root"]["children"][0]["children"][1], original["root"]["children"][0]["children"][1]);
    assert_eq!(s.to_plain_text().replace('\n', ""), "a");
}

#[test]
fn editing_around_unmodelled_nodes_keeps_them() {
    use lexical_core::{Command, Editor};
    let doc = r#"{"root":{"type":"root","children":[
      {"type":"paragraph","children":[{"type":"text","text":"ab","format":0,"style":"","mode":"normal","detail":0},{"type":"mention","name":"m"}]},
      {"type":"table","rows":[]},
      {"type":"paragraph","children":[{"type":"text","text":"cd","format":0,"style":"","mode":"normal","detail":0}]}]}}"#;
    let count = |e: &Editor| e.state().to_json_string().matches("\"mention\"").count()
        + e.state().to_json_string().matches("\"table\"").count();
    let mut e = Editor::with_state(EditorState::from_json_str(doc).unwrap());
    assert_eq!(count(&e), 2);
    for c in [
        Command::InsertText("x".into()),
        Command::InsertParagraph,
        Command::DeleteCharacter { backward: true },
        Command::DeleteCharacter { backward: false },
        Command::Undo,
    ] {
        e.dispatch(c);
        e.state().check_invariants().expect("invariants");
        assert_eq!(count(&e), 2, "unmodelled node lost");
    }
}
