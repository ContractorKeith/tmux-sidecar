#[test]
fn documented_methods_match_the_wire_table_exactly() {
    let docs = include_str!("../docs/rpc.md");
    let headings: std::collections::BTreeSet<_> =
        docs.lines().filter_map(|l| l.strip_prefix("## ")).collect();
    let methods = tmux_sidecar::rpc::METHODS.iter().copied().collect();
    assert_eq!(headings, methods);
}
