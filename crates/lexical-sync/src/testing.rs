//! Test support: a cluster of in-memory replicas with controllable delivery, and
//! deterministic randomness for fuzzing editing sessions.
//!
//! Real networks drop, duplicate, delay and reorder messages. The CRDT underneath has to
//! cope, and tests use this to check that it does.

use crate::collab::Replica;
use crate::flat::flatten;
use lexical_core::{
    Align, BlockType, Command, Editor, HeadingTag, Layout, ListType, TextFormat,
};

pub struct Cluster {
    pub replicas: Vec<Replica>,
}

impl Cluster {
    /// `n` replicas (peers 1..=n) starting from the shared initial document.
    pub fn new(n: usize) -> Cluster {
        Cluster { replicas: (1..=n as u64).map(|p| Replica::new(p).expect("replica")).collect() }
    }

    /// Updates replica `i` has produced since the last call.
    pub fn take_outbox(&self, i: usize) -> Vec<Vec<u8>> {
        self.replicas[i].drain_updates()
    }

    pub fn deliver(&mut self, to: usize, messages: &[Vec<u8>]) {
        for m in messages {
            self.replicas[to].receive(m).expect("receive");
        }
    }

    /// Deliver everything every replica has produced to every other replica, in order.
    pub fn sync_all(&mut self) {
        let outboxes: Vec<Vec<Vec<u8>>> = (0..self.replicas.len()).map(|i| self.take_outbox(i)).collect();
        for (from, msgs) in outboxes.iter().enumerate() {
            for to in (0..self.replicas.len()).filter(|&t| t != from) {
                self.deliver(to, msgs);
            }
        }
    }

    /// Whether every replica holds the same document. Compared in the flattened form:
    /// two trees can differ structurally (say, two adjacent links to the same URL) while
    /// representing the same text, formats and blocks.
    pub fn converged(&self) -> bool {
        let first = flatten(self.replicas[0].editor.state());
        self.replicas.iter().all(|r| flatten(r.editor.state()) == first)
    }

    /// Anti-entropy: every replica sends its version to every other and receives what
    /// it is missing. Recovers from dropped messages.
    pub fn anti_entropy(&mut self) {
        let n = self.replicas.len();
        for a in 0..n {
            for b in 0..n {
                if a == b {
                    continue;
                }
                let missing = {
                    let version_of_b = self.replicas[b].collab.version();
                    self.replicas[a].collab.updates_since(&version_of_b).expect("updates")
                };
                self.replicas[b].receive(&missing).expect("receive");
            }
        }
        for r in &self.replicas {
            r.drain_updates();
        }
    }

    /// Panic with a description if any replica violates a document invariant.
    pub fn assert_valid(&self) {
        for (i, r) in self.replicas.iter().enumerate() {
            if let Err(msg) = r.editor.state().check_invariants() {
                panic!("replica {i} violates an invariant: {msg}\n{}", r.editor.state().to_json_string());
            }
        }
    }
}

/// xorshift64: small, dependency-free, reproducible from a seed.
pub struct Rng(pub u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n.max(1) as u64) as usize
    }

    pub fn chance(&mut self, one_in: usize) -> bool {
        self.below(one_in) == 0
    }
}

const WORDS: &[&str] = &["a", "bc", "def ", " ", "héllo", "👨‍👩‍👧", "x y z", "ß"];

/// A random editing command, covering typing, structure, formatting and history.
pub fn random_command(r: &mut Rng) -> Command {
    match r.below(22) {
        0..=4 => Command::InsertText(WORDS[r.below(WORDS.len())].into()),
        5 => Command::InsertParagraph,
        6 => Command::InsertLineBreak,
        7 | 8 => Command::DeleteCharacter { backward: r.chance(2) },
        9 => Command::DeleteWord { backward: r.chance(2) },
        10 => Command::DeleteLine { backward: r.chance(2) },
        11 => Command::FormatText(
            [TextFormat::BOLD, TextFormat::ITALIC, TextFormat::CODE, TextFormat::SUBSCRIPT, TextFormat::SUPERSCRIPT]
                [r.below(5)],
        ),
        12 => Command::SetBlockType(
            [BlockType::Paragraph, BlockType::Quote, BlockType::Code, BlockType::Heading(HeadingTag::H2)][r.below(4)],
        ),
        13 => Command::ToggleList([ListType::Bullet, ListType::Number, ListType::Check][r.below(3)]),
        14 => Command::ToggleLink(if r.chance(3) { None } else { Some("https://x.test".into()) }),
        15 => Command::Indent,
        16 => Command::Outdent,
        17 => Command::Paste(format!("{}\n{}\n{}", WORDS[r.below(8)], WORDS[r.below(8)], WORDS[r.below(8)])),
        18 => Command::Undo,
        19 => Command::Redo,
        20 => Command::ToggleCheck,
        _ => Command::FormatAlign([Align::Left, Align::Center, Align::Right][r.below(3)]),
    }
}

/// Move the editor's selection to a random place (sometimes a range).
pub fn random_selection(editor: &mut Editor, r: &mut Rng) {
    let state = editor.state();
    let layout = Layout::build(state);
    let n = layout.char_len() + 1;
    let a = r.below(n);
    let f = if r.chance(2) { r.below(n) } else { a };
    let (pa, pf) = (layout.point_at(state, a), layout.point_at(state, f));
    editor.set_selection(pa, pf);
}
