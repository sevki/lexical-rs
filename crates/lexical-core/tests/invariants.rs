//! Randomised trace testing, in the spirit of the Verus `Inv` obligations: whatever
//! sequence of commands, selection moves, undo/redo and reloads happens, every
//! committed state satisfies `check_invariants`, and serialisation is lossless.

use lexical_core::*;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

const WORDS: &[&str] = &["a", "bc", "def ", " ", "héllo", "👨‍👩‍👧", "x y z", "ß"];

fn random_command(r: &mut Rng) -> Command {
    match r.below(22) {
        0..=4 => Command::InsertText(WORDS[r.below(WORDS.len())].into()),
        5 => Command::InsertParagraph,
        6 => Command::InsertLineBreak,
        7 | 8 => Command::DeleteCharacter { backward: r.below(2) == 0 },
        9 => Command::DeleteWord { backward: r.below(2) == 0 },
        10 => Command::DeleteLine { backward: r.below(2) == 0 },
        11 => Command::FormatText([TextFormat::BOLD, TextFormat::ITALIC, TextFormat::CODE, TextFormat::SUBSCRIPT, TextFormat::SUPERSCRIPT][r.below(5)]),
        12 => Command::SetBlockType(
            [BlockType::Paragraph, BlockType::Quote, BlockType::Code, BlockType::Heading(HeadingTag::H2)][r.below(4)],
        ),
        13 => Command::ToggleList([ListType::Bullet, ListType::Number, ListType::Check][r.below(3)]),
        14 => Command::ToggleLink(if r.below(3) == 0 { None } else { Some("https://x.test".into()) }),
        15 => Command::Indent,
        16 => Command::Outdent,
        17 => Command::Paste(format!("{}\n{}\n{}", WORDS[r.below(8)], WORDS[r.below(8)], WORDS[r.below(8)])),
        18 => Command::Undo,
        19 => Command::Redo,
        20 => Command::ToggleCheck,
        _ => Command::FormatAlign([Align::Left, Align::Center, Align::Right][r.below(3)]),
    }
}

fn random_selection(e: &mut Editor, r: &mut Rng) {
    let s = e.state();
    let l = Layout::build(s);
    let n = l.char_len() + 1;
    let (a, f) = (r.below(n), if r.below(2) == 0 { r.below(n) } else { 0 });
    let (a, f) = if f == 0 { (a, a) } else { (a, f) };
    let (pa, pf) = (l.point_at(s, a), l.point_at(s, f));
    e.set_selection(pa, pf);
}

fn run_trace(seed: u64, steps: usize, markdown: bool) {
    let mut r = Rng(seed | 1);
    let mut e = Editor::new();
    if markdown {
        e.add_plugin(Box::new(MarkdownShortcutsPlugin::default()));
    }
    let mut log: Vec<String> = vec![];
    for step in 0..steps {
        let before = format!("{:?}\n{}", e.state().selection, e.state().to_json_string());
        if r.below(4) == 0 {
            random_selection(&mut e, &mut r);
            log.push("select".into());
        } else {
            let c = random_command(&mut r);
            log.push(format!("{c:?}"));
            e.dispatch(c);
        }
        if let Err(msg) = e.state().check_invariants() {
            panic!(
                "seed {seed} step {step}: {msg}\nlast commands: {:#?}\nstate: {}\nBEFORE LAST STEP:\n{before}",
                &log[log.len().saturating_sub(8)..],
                e.state().to_json_string()
            );
        }
        // Serialisation is lossless at every step (selection excluded).
        if step % 7 == 0 {
            let json = e.state().to_json();
            let back = EditorState::from_json(&json).expect("reload");
            back.check_invariants().unwrap_or_else(|m| panic!("seed {seed} step {step}: reload: {m}"));
            assert_eq!(back.to_json(), json, "seed {seed} step {step}: JSON not stable");
        }
    }
}

/// Number of random traces; raise with `LEXICAL_FUZZ_SEEDS=5000` for a deeper sweep.
fn seeds(default: u64) -> u64 {
    std::env::var("LEXICAL_FUZZ_SEEDS").ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

#[test]
fn random_traces_preserve_invariants() {
    for seed in 1..=seeds(300) {
        run_trace(seed.wrapping_mul(0x9E3779B97F4A7C15), 120, false);
    }
}

#[test]
fn random_traces_with_markdown_plugin() {
    for seed in 1..=seeds(100) {
        run_trace(seed.wrapping_mul(0xD1B54A32D192ED03), 120, true);
    }
}

#[test]
fn undo_everything_returns_to_the_start() {
    for seed in 1..=50u64 {
        let mut r = Rng(seed.wrapping_mul(0xA24BAED4963EE407) | 1);
        let mut e = Editor::new();
        let initial = e.state().to_json();
        for _ in 0..60 {
            let c = random_command(&mut r);
            if matches!(c, Command::Undo | Command::Redo) {
                continue;
            }
            e.dispatch(c);
        }
        while e.undo() {}
        assert_eq!(e.state().to_json(), initial, "seed {seed}");
        e.state().check_invariants().unwrap();
    }
}
