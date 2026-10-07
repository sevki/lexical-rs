//! Randomised collaboration sessions: several peers edit concurrently while the network
//! delays, reorders and duplicates their updates. After every step each replica must be a
//! valid document; once everything is delivered all replicas must be identical.
//!
//! `LEXICAL_FUZZ_SEEDS=500 cargo test --release -p lexical-sync --test fuzz` for a deeper sweep.

use lexical_sync::testing::{random_command, random_selection, Cluster, Rng};

fn seeds(default: u64) -> u64 {
    std::env::var("LEXICAL_FUZZ_SEEDS").ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn session(seed: u64, peers: usize, steps: usize) {
    let mut c = Cluster::new(peers);
    let mut r = Rng::new(seed);
    // updates in flight towards each replica
    let mut inbox: Vec<Vec<Vec<u8>>> = vec![vec![]; peers];
    let mut log: Vec<String> = vec![];

    let publish = |c: &Cluster, inbox: &mut Vec<Vec<Vec<u8>>>, from: usize| {
        for m in c.take_outbox(from) {
            for (to, queue) in inbox.iter_mut().enumerate() {
                if to != from {
                    queue.push(m.clone());
                }
            }
        }
    };

    for step in 0..steps {
        match r.below(10) {
            0..=5 => {
                let who = r.below(peers);
                if r.chance(5) {
                    random_selection(&mut c.replicas[who].editor, &mut r);
                    log.push(format!("{who}: select"));
                } else {
                    let cmd = random_command(&mut r);
                    log.push(format!("{who}: {cmd:?}"));
                    c.replicas[who].dispatch(cmd);
                }
            }
            6..=8 => {
                let to = r.below(peers);
                if !inbox[to].is_empty() {
                    let pick = r.below(inbox[to].len());
                    let m = inbox[to].swap_remove(pick);
                    if r.chance(5) {
                        inbox[to].push(m.clone()); // the network duplicates it
                    }
                    log.push(format!("{to}: receive"));
                    c.replicas[to].receive(&m).unwrap_or_else(|e| panic!("seed {seed} step {step}: {e}"));
                }
            }
            _ => {
                let from = r.below(peers);
                publish(&c, &mut inbox, from);
            }
        }
        for (i, rep) in c.replicas.iter().enumerate() {
            if let Err(msg) = rep.editor.state().check_invariants() {
                panic!(
                    "seed {seed} step {step}: replica {i}: {msg}\nlast steps: {:#?}\n{}",
                    &log[log.len().saturating_sub(6)..],
                    rep.editor.state().to_json_string()
                );
            }
        }
    }

    // drain: publish everything, then deliver in a random order
    for from in 0..peers {
        publish(&c, &mut inbox, from);
    }
    for (to, queue) in inbox.iter_mut().enumerate() {
        while !queue.is_empty() {
            let pick = r.below(queue.len());
            let m = queue.swap_remove(pick);
            c.replicas[to].receive(&m).unwrap_or_else(|e| panic!("seed {seed} drain: {e}"));
        }
    }
    c.assert_valid();
    assert!(c.converged(), "seed {seed}: replicas diverged after full delivery\nlast steps: {:#?}", &log[log.len().saturating_sub(8)..]);
    for r in &c.replicas {
        assert!(r.collab.take_errors().is_empty(), "seed {seed}: a local edit failed to reach the CRDT");
    }
}

#[test]
fn three_peers_with_a_lossless_but_chaotic_network() {
    for seed in 1..=seeds(40) {
        session(seed, 3, 110);
    }
}

#[test]
fn two_peers_hammering_the_same_paragraph() {
    for seed in 1..=seeds(60) {
        session(seed.wrapping_mul(7919), 2, 90);
    }
}

#[test]
fn five_peers() {
    for seed in 1..=seeds(10) {
        session(seed.wrapping_mul(104729), 5, 120);
    }
}
