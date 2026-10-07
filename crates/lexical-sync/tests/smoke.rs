use lexical_core::Command;
use lexical_sync::testing::Cluster;

#[test]
fn two_replicas_type_concurrently_and_converge() {
    let mut c = Cluster::new(2);
    c.replicas[0].dispatch(Command::InsertText("hello".into()));
    c.replicas[1].dispatch(Command::InsertText("world".into()));
    assert!(!c.converged());
    c.sync_all();
    assert!(c.converged(), "{:#?} vs {:#?}", c.replicas[0].json(), c.replicas[1].json());
    let t = c.replicas[0].text();
    assert!(t.contains("hello") && t.contains("world"), "{t:?}");
    println!("merged: {t:?}");
}
