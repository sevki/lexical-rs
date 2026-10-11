//! Conformance against the real thing: Lexical for JavaScript with `@lexical/yjs`.
//!
//! The JavaScript side is `plugins/lexical-js-shim/yjs-interop.mjs`. These tests need
//! `npm ci` in that directory; without it they are skipped, unless `LEXICAL_JS_REQUIRED` is
//! set (CI sets it).

use lexical_core::{Command, Editor, EditorState};
use lexical_yjs::YjsDoc;
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command as Process;

fn shim_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/lexical-js-shim")
}

fn js_available() -> bool {
    let ok = shim_dir().join("node_modules/@lexical/yjs").exists();
    if !ok {
        assert!(
            std::env::var_os("LEXICAL_JS_REQUIRED").is_none(),
            "LEXICAL_JS_REQUIRED is set but plugins/lexical-js-shim has no node_modules"
        );
        eprintln!("skipped: run `npm ci` in plugins/lexical-js-shim");
    }
    ok
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
}

/// Run `yjs-interop.mjs`; returns its `{update, json}` output.
fn js(cmd: &str, update: Option<&[u8]>) -> (Vec<u8>, Value) {
    let mut args = vec!["yjs-interop.mjs".to_string(), cmd.to_string()];
    let file = update.map(|u| {
        // Tests run in parallel: one file per call.
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("lexical-yjs-{}-{n}.hex", std::process::id()));
        std::fs::write(&p, hex(u)).unwrap();
        p
    });
    if let Some(f) = &file {
        args.push(f.display().to_string());
    }
    let out = Process::new("node").args(&args).current_dir(shim_dir()).output().expect("node");
    if let Some(f) = &file {
        let _ = std::fs::remove_file(f);
    }
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    (unhex(v["update"].as_str().unwrap()), v["json"].clone())
}

/// Both sides through the engine's own JSON, so only what the engine models is compared.
fn canon(v: &Value) -> Value {
    EditorState::from_json(v).expect("valid").to_json()
}

fn diff(a: &Value, b: &Value, path: String, out: &mut Vec<String>) {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for k in x.keys().chain(y.keys()).collect::<std::collections::BTreeSet<_>>() {
                diff(x.get(k).unwrap_or(&Value::Null), y.get(k).unwrap_or(&Value::Null), format!("{path}/{k}"), out);
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                diff(p, q, format!("{path}[{i}]"), out);
            }
        }
        _ if a != b => out.push(format!("{path}: {a} != {b}")),
        _ => {}
    }
}

/// Drop what Lexical for JavaScript derives from the tree instead of storing: a
/// paragraph's `textFormat` / `textStyle` follow its text, a list item's indent its nesting.
fn strip_derived(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let item = m.get("type").and_then(Value::as_str) == Some("listitem");
            let mut out = serde_json::Map::new();
            for (k, x) in m {
                if matches!(k.as_str(), "textFormat" | "textStyle") || (item && k == "indent") {
                    continue;
                }
                out.insert(k.clone(), strip_derived(x));
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(strip_derived).collect()),
        x => x.clone(),
    }
}

fn assert_same(a: &Value, b: &Value) {
    let mut out = vec![];
    diff(&strip_derived(a), &strip_derived(b), String::new(), &mut out);
    assert!(out.is_empty(), "documents differ:\n{}", out.join("\n"));
}

fn doc_from(update: &[u8]) -> YjsDoc {
    let d = YjsDoc::new();
    d.apply_update(update).unwrap();
    d
}

#[test]
fn reads_what_lexical_js_wrote() {
    if !js_available() {
        return;
    }
    let (update, json) = js("gen", None);
    let state = doc_from(&update).state().expect("readable");
    assert_same(&state.to_json(), &canon(&json));
    assert!(state.to_plain_text().contains("\u{1F600}"));
}

#[test]
fn lexical_js_reads_what_rust_wrote() {
    if !js_available() {
        return;
    }
    let (_, json) = js("gen", None);
    let state = EditorState::from_json(&json).unwrap();
    let doc = YjsDoc::new();
    let update = doc.set_state(&state).unwrap();
    // Round trip inside Rust, then through the real binding.
    assert_eq!(doc.state().unwrap().to_json(), state.to_json());
    let (_, loaded) = js("load", Some(&update));
    assert_same(&canon(&loaded), &state.to_json());
}

#[test]
fn rust_edits_reach_lexical_js_as_small_updates() {
    if !js_available() {
        return;
    }
    let (update, _) = js("gen", None);
    let doc = doc_from(&update);
    let mut editor = Editor::with_state(doc.state().unwrap());
    editor.dispatch(Command::InsertText("RUST".into()));
    let delta = doc.set_state(editor.state()).unwrap();
    // One new text node, not the document (which is several KB).
    let full = doc.encode_state().len();
    assert!(delta.len() * 10 < full, "typing sent {} of {full} bytes", delta.len());
    // A second set_state with nothing changed adds nothing.
    let before = doc.state_vector();
    doc.set_state(editor.state()).unwrap();
    assert_eq!(doc.state_vector(), before);

    let (_, loaded) = js("load", Some(&doc.encode_state()));
    assert_same(&canon(&loaded), &editor.state().to_json());
    assert!(loaded.to_string().contains("RUST"));
}

#[test]
fn lexical_js_edits_reach_rust() {
    if !js_available() {
        return;
    }
    let (update, _) = js("gen", None);
    let (edited, json) = js("edit", Some(&update));
    let doc = doc_from(&update);
    doc.apply_update(&edited).unwrap();
    let state = doc.state().unwrap();
    assert_same(&state.to_json(), &canon(&json));
    assert!(state.to_plain_text().contains("hi js"));
}

#[test]
fn structural_edits_round_trip_through_lexical_js() {
    if !js_available() {
        return;
    }
    let (update, _) = js("gen", None);
    let doc = doc_from(&update);
    let mut editor = Editor::with_state(doc.state().unwrap());
    editor.dispatch(Command::InsertParagraph);
    editor.dispatch(Command::InsertText("new block".into()));
    editor.dispatch(Command::FormatText(lexical_core::TextFormat::BOLD));
    doc.set_state(editor.state()).unwrap();
    assert_same(&doc.state().unwrap().to_json(), &editor.state().to_json());
    let (_, loaded) = js("load", Some(&doc.encode_state()));
    assert_same(&canon(&loaded), &editor.state().to_json());
}

#[test]
fn concurrent_edits_from_both_sides_merge() {
    if !js_available() {
        return;
    }
    let (base, _) = js("gen", None);
    // JavaScript edits the base while Rust edits the same base.
    let (js_side, _) = js("edit", Some(&base));
    let doc = doc_from(&base);
    let mut editor = Editor::with_state(doc.state().unwrap());
    editor.dispatch(Command::InsertText("RUST".into()));
    doc.set_state(editor.state()).unwrap();
    doc.apply_update(&js_side).unwrap();

    let merged = doc.state().unwrap();
    let text = merged.to_plain_text();
    assert!(text.contains("RUST") && text.contains("hi js"), "{text:?}");
    // Lexical for JavaScript, given the merged document, sees the same thing.
    let (_, seen_by_js) = js("load", Some(&doc.encode_state()));
    assert_same(&canon(&seen_by_js), &merged.to_json());
}
