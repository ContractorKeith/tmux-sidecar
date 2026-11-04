#[test]
fn documented_methods_match_the_wire_table_exactly() {
    let docs = include_str!("../docs/rpc.md");
    let headings: std::collections::BTreeSet<_> =
        docs.lines().filter_map(|l| l.strip_prefix("## ")).collect();
    let methods = tmux_sidecar::rpc::METHODS.iter().copied().collect();
    assert_eq!(headings, methods);
}

#[test]
fn every_documented_method_has_a_dispatch_branch() {
    use tmux_sidecar::{daemon, rpc, store::Store};
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(&root.path().join("methods.db")).unwrap();
    for method in rpc::METHODS {
        let request = rpc::Request {
            jsonrpc: rpc::Version::V2,
            id: rpc::Id::Number(1),
            method: (*method).into(),
            params: None,
        };
        if let rpc::Outcome::Failure { error } =
            daemon::dispatch(request, &store, "test", 1000).outcome
        {
            assert_ne!(
                error.message, "unknown method",
                "missing dispatch for {method}"
            );
        }
    }
}
