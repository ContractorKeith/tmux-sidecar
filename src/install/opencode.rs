use std::io;
pub const MARKER: &str = "// tmux-sidecar: managed plugin";
const END: &str = "// tmux-sidecar: end";
pub fn plugin(binary: &str) -> String {
    let binary = serde_json::to_string(binary).expect("string serialization");
    format!(
        r#"{MARKER}
import {{ spawn }} from "node:child_process";
export default {{
  id: "tmux-sidecar",
  setup(ctx) {{
    const stop = new AbortController();
    const pending = new Set();
    void (async () => {{
      for await (const event of ctx.event.subscribe({{ signal: stop.signal }})) {{
        if (!["session.status", "session.idle", "session.error", "session.execution.started", "session.execution.succeeded", "session.execution.failed", "session.execution.interrupted", "permission.asked", "permission.replied"].includes(event.type)) continue;
        try {{
          const child = spawn({binary}, ["hook", "opencode"], {{ detached: true, stdio: ["pipe", "ignore", "ignore"] }});
          const done = new Promise(resolve => {{ child.once("error", resolve); child.once("close", resolve); }});
          pending.add(done);
          done.finally(() => pending.delete(done));
          child.stdin.on("error", () => {{}});
          child.stdin.end(JSON.stringify({{ ...event, cwd: ctx.location.directory }}));
          child.unref();
        }} catch {{}}
      }}
    }})().catch(() => {{}});
    return async () => {{
      stop.abort();
      await Promise.race([Promise.allSettled([...pending]), new Promise(resolve => setTimeout(resolve, 250))]);
    }};
  }}
}};
{END}
"#
    )
}
pub fn edit(text: &str, install: bool, binary: &str) -> io::Result<String> {
    if !text.is_empty() && !text.starts_with(MARKER) {
        return if install {
            Err(io::Error::other(
                "refusing to overwrite unmarked OpenCode plugin",
            ))
        } else {
            Ok(text.to_owned())
        };
    }
    let suffix = if let Some((_, suffix)) = text.split_once(END) {
        suffix.strip_prefix('\n').unwrap_or(suffix)
    } else if let Some((_, suffix)) = text.split_once("\n});\n") {
        suffix
    } else {
        ""
    };
    Ok(format!(
        "{}{suffix}",
        if install {
            plugin(binary)
        } else {
            String::new()
        }
    ))
}
