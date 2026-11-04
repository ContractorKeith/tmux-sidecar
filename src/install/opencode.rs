pub const MARKER: &str = "// tmux-sidecar: managed plugin";
pub fn plugin(binary: &str) -> String {
    let binary = serde_json::to_string(binary).expect("string serialization");
    format!(
        r#"{MARKER}
import {{ spawn }} from "node:child_process";
export const TmuxSidecar = async ({{ directory }}) => ({{
  event: async ({{ event }}) => {{
    if (!["session.status", "session.idle", "session.error", "permission.asked", "permission.replied"].includes(event.type)) return;
    try {{
      const child = spawn({binary}, ["hook", "opencode"], {{ stdio: ["pipe", "ignore", "ignore"] }});
      child.on("error", () => {{}});
      child.stdin.on("error", () => {{}});
      child.stdin.end(JSON.stringify({{ ...event, cwd: directory }}));
      child.unref();
    }} catch {{}}
  }}
}});
"#
    )
}
