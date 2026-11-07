use super::{
    TmuxGuard, enabled,
    lifecycle::{list, stop},
    mcp_client::Mcp,
    mcp_replies,
};
use serde_json::json;

e2e_test!(mcp_fake_replies_match_real_wait_and_send_contract, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start_capture();
    let run = list(&guard)[0].clone();
    super::cli_verbs::fire(&guard, &run, "Stop");
    let mut mcp = Mcp::start(&guard, true);
    let actual = mcp.json(
        "wait_for_state",
        json!({"selector":run["id"],"state":"waiting","timeout":1}),
    );
    assert_eq!(
        actual,
        mcp_replies::waiting(
            run["id"].clone(),
            run["state"].clone(),
            run["state_since"].clone()
        )
    );
    assert_eq!(
        mcp.json(
            "send_prompt",
            json!({"selector":run["id"],"text":"contract probe"})
        ),
        mcp_replies::sent()
    );
    drop(mcp);
    stop(&guard);
});
