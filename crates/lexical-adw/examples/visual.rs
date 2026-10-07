//! Visual regression tool.
//!
//! ```text
//! visual render <out-dir>                       draw every scenario to <out-dir>/<name>.png
//! visual compare <base-dir> <head-dir> <diff> [report.md]   diff two render directories
//! ```
//!
//! CI renders the base branch and the PR head in the same job (same fonts, same GTK) and
//! compares them, so there are no committed baselines to go stale. `compare` exits
//! non-zero when any scenario differs or was removed, writes a red-highlighted diff image
//! per changed scenario, and emits a markdown report (to `report.md`, or stdout).
//!
//! Rendering is made deterministic: fixed font, light style, no animations, no caret blink.
//! Run it under a display server, e.g. `xvfb-run -a dbus-run-session -- ...`.

use adw::gtk::{self, gdk, gio, glib, prelude::*};
use lexical_adw::LexicalView;
use lexical_core::{BlockType, Command, HeadingTag, ListType, TextFormat};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

const WIDTH: i32 = 720;
const HEIGHT: i32 = 520;
/// Per-channel difference below which two pixels count as equal (anti-aliasing noise).
const CHANNEL_TOLERANCE: u8 = 2;

type Scenario = (&'static str, fn(&LexicalView));

const SCENARIOS: &[Scenario] = &[
    ("empty", |_| {}),
    ("paragraphs-and-formats", paragraphs_and_formats),
    ("headings-quote-code", headings_quote_code),
    ("lists", lists),
    ("selection-and-toolbar", selection_and_toolbar),
];

fn select_offsets(v: &LexicalView, from: i32, to: i32) {
    v.buffer().select_range(&v.buffer().iter_at_offset(from), &v.buffer().iter_at_offset(to));
}

/// Select whole lines `first..=last` (line numbers stay valid when list markers are added).
fn select_lines(v: &LexicalView, first: i32, last: i32) {
    let start = v.buffer().iter_at_line(first).expect("line exists");
    let mut end = v.buffer().iter_at_line(last).expect("line exists");
    end.forward_to_line_end();
    v.buffer().select_range(&start, &end);
}

fn paragraphs_and_formats(v: &LexicalView) {
    // "Plain text, then bold, italic, underline, strikethrough and code."
    v.dispatch(Command::Paste(
        "Plain text, then bold, italic, underline, strikethrough and code.\nA second paragraph that is long enough to wrap onto another line so wrapping and spacing are covered by the snapshot.".into(),
    ));
    let mark = |from, to, cmd| {
        select_offsets(v, from, to);
        v.dispatch(cmd);
    };
    mark(17, 21, Command::FormatText(TextFormat::BOLD));
    mark(23, 29, Command::FormatText(TextFormat::ITALIC));
    mark(31, 40, Command::FormatText(TextFormat::UNDERLINE));
    mark(42, 55, Command::FormatText(TextFormat::STRIKETHROUGH));
    mark(60, 64, Command::FormatText(TextFormat::CODE));
    mark(6, 10, Command::ToggleLink(Some("https://lexical.dev".into())));
    select_offsets(v, 0, 0);
}

fn headings_quote_code(v: &LexicalView) {
    v.dispatch(Command::Paste("Heading one\nHeading two\nA quoted line\nlet x = 1;\nBack to a paragraph.".into()));
    for (line, ty) in [
        (0, BlockType::Heading(HeadingTag::H1)),
        (1, BlockType::Heading(HeadingTag::H2)),
        (2, BlockType::Quote),
        (3, BlockType::Code),
    ] {
        select_lines(v, line, line);
        v.dispatch(Command::SetBlockType(ty));
    }
    select_offsets(v, 0, 0);
}

fn lists(v: &LexicalView) {
    v.dispatch(Command::Paste("Bullet one\nBullet two\nNested item\nNumber one\nNumber two\nTask open\nTask done".into()));
    select_lines(v, 0, 2);
    v.dispatch(Command::ToggleList(ListType::Bullet));
    select_lines(v, 2, 2);
    v.dispatch(Command::Indent);
    select_lines(v, 3, 4);
    v.dispatch(Command::ToggleList(ListType::Number));
    select_lines(v, 5, 6);
    v.dispatch(Command::ToggleList(ListType::Check));
    select_lines(v, 6, 6);
    v.dispatch(Command::ToggleCheck);
    select_offsets(v, 0, 0);
}

fn selection_and_toolbar(v: &LexicalView) {
    v.dispatch(Command::Paste("Select some of this text to see the toolbar state and selection colour.".into()));
    v.dispatch(Command::SelectAll);
    v.dispatch(Command::FormatText(TextFormat::BOLD));
    v.buffer().select_range(&v.buffer().iter_at_offset(7), &v.buffer().iter_at_offset(24));
}

fn pump(ms: u64) {
    let main_loop = glib::MainLoop::new(None, false);
    let quit = main_loop.clone();
    glib::timeout_add_local_once(Duration::from_millis(ms), move || quit.quit());
    main_loop.run();
}

fn make_deterministic() {
    let settings = gtk::Settings::default().expect("display");
    settings.set_gtk_font_name(Some("DejaVu Sans 11"));
    settings.set_gtk_enable_animations(false);
    settings.set_gtk_cursor_blink(false);
    settings.set_gtk_theme_name(Some("Adwaita"));
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceLight);
}

fn render(out: &Path) -> Result<(), String> {
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    for (name, setup) in SCENARIOS {
        let view = LexicalView::new();
        let toolbar_view = adw::ToolbarView::new();
        toolbar_view.add_top_bar(view.toolbar());
        toolbar_view.set_content(Some(view.widget()));
        let window = adw::Window::builder()
            .default_width(WIDTH)
            .default_height(HEIGHT)
            .resizable(false)
            .content(&toolbar_view)
            .build();
        window.present();
        pump(300);
        setup(&view);
        pump(400);
        let path = out.join(format!("{name}.png"));
        snapshot_png(toolbar_view.upcast_ref(), &path)?;
        println!("rendered {}", path.display());
        window.close();
        pump(100);
    }
    Ok(())
}

fn snapshot_png(widget: &gtk::Widget, path: &Path) -> Result<(), String> {
    let (w, h) = (widget.width(), widget.height());
    if w == 0 || h == 0 {
        return Err("widget has no size; is a display available?".into());
    }
    let paintable = gtk::WidgetPaintable::new(Some(widget));
    let snapshot = gtk::Snapshot::new();
    paintable.snapshot(&snapshot, f64::from(w), f64::from(h));
    let node = snapshot.to_node().ok_or("nothing was drawn")?;
    let renderer = widget
        .native()
        .and_then(|n| n.renderer())
        .ok_or("widget is not realized")?;
    let texture = renderer.render_texture(&node, None);
    texture.save_to_png(path).map_err(|e| e.to_string())
}

struct Image {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

fn load(path: &Path) -> Result<Image, String> {
    let texture = gdk::Texture::from_file(&gio::File::for_path(path)).map_err(|e| e.to_string())?;
    let (width, height) = (texture.width() as usize, texture.height() as usize);
    let stride = width * 4;
    let mut pixels = vec![0u8; stride * height];
    texture.download(&mut pixels, stride);
    Ok(Image { width, height, pixels })
}

/// Number of differing pixels, plus a diff image (changed pixels red over a faded copy).
fn diff(a: &Image, b: &Image) -> (usize, Vec<u8>) {
    let (w, h) = (a.width.max(b.width), a.height.max(b.height));
    let mut out = vec![0u8; w * h * 4];
    let mut changed = 0;
    for y in 0..h {
        for x in 0..w {
            let at = |img: &Image| {
                (x < img.width && y < img.height).then(|| {
                    let i = (y * img.width + x) * 4;
                    [img.pixels[i], img.pixels[i + 1], img.pixels[i + 2], img.pixels[i + 3]]
                })
            };
            let (pa, pb) = (at(a), at(b));
            let o = (y * w + x) * 4;
            let same = match (pa, pb) {
                (Some(p), Some(q)) => p.iter().zip(q.iter()).all(|(l, r)| l.abs_diff(*r) <= CHANNEL_TOLERANCE),
                _ => false,
            };
            if same {
                let p = pa.unwrap();
                // faded, so changes stand out
                out[o..o + 4].copy_from_slice(&[p[0] / 3 + 170, p[1] / 3 + 170, p[2] / 3 + 170, 255]);
            } else {
                changed += 1;
                // B8G8R8A8: blue, green, red, alpha
                out[o..o + 4].copy_from_slice(&[40, 40, 230, 255]);
            }
        }
    }
    (changed, out)
}

fn save_diff(pixels: &[u8], w: usize, h: usize, path: &Path) -> Result<(), String> {
    let bytes = glib::Bytes::from(pixels);
    let texture = gdk::MemoryTexture::new(w as i32, h as i32, gdk::MemoryFormat::B8g8r8a8, &bytes, w * 4);
    texture.save_to_png(path).map_err(|e| e.to_string())
}

fn pngs(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| n.ends_with(".png"))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// Diff two render directories. The markdown report goes to `report` (stdout when
/// `None`); returns whether everything matched.
fn compare(base: &Path, head: &Path, diff_dir: &Path, report: Option<&Path>) -> Result<bool, String> {
    std::fs::create_dir_all(diff_dir).map_err(|e| e.to_string())?;
    let (base_names, head_names) = (pngs(base), pngs(head));
    let mut ok = true;
    let mut rows = String::from("| scenario | result |\n|---|---|\n");
    for name in &head_names {
        let scenario = name.trim_end_matches(".png");
        if !base_names.contains(name) {
            rows += &format!("| `{scenario}` | new scenario (nothing to compare) |\n");
            continue;
        }
        let (a, b) = (load(&base.join(name))?, load(&head.join(name))?);
        let (changed, pixels) = diff(&a, &b);
        if changed == 0 {
            rows += &format!("| `{scenario}` | unchanged |\n");
        } else {
            ok = false;
            let (w, h) = (a.width.max(b.width), a.height.max(b.height));
            save_diff(&pixels, w, h, &diff_dir.join(name))?;
            rows += &format!(
                "| `{scenario}` | **changed**: {changed} px ({:.2}%), see `diff/{name}` |\n",
                changed as f64 * 100.0 / (w * h) as f64
            );
        }
    }
    for name in base_names.iter().filter(|n| !head_names.contains(n)) {
        ok = false;
        rows += &format!("| `{}` | **removed** |\n", name.trim_end_matches(".png"));
    }
    match report {
        Some(path) => std::fs::write(path, &rows).map_err(|e| e.to_string())?,
        None => print!("{rows}"),
    }
    Ok(ok)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if gtk::init().is_err() || adw::init().is_err() {
        eprintln!("no display available; run under xvfb-run");
        return ExitCode::from(2);
    }
    make_deterministic();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["render", out] => render(&PathBuf::from(out)).map(|()| true),
        ["compare", base, head, diff] => compare(Path::new(base), Path::new(head), Path::new(diff), None),
        ["compare", base, head, diff, report] => {
            compare(Path::new(base), Path::new(head), Path::new(diff), Some(Path::new(report)))
        }
        _ => Err("usage: visual render <out-dir> | visual compare <base-dir> <head-dir> <diff-dir> [report.md]".into()),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}
