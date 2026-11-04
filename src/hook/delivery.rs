use super::{
    signal::{self, Signal},
    spool,
};
use crate::{client, paths, proc, seams::ProcessTree};
use std::{
    io::{self, Read},
    path::Path,
    time::Duration,
};
use tokio::io::AsyncWriteExt;

pub async fn run(args: Vec<String>) -> io::Result<()> {
    let Some(tmux) = std::env::var_os("TMUX") else {
        return Ok(());
    };
    let Some(pane) = std::env::var("TMUX_PANE").ok().filter(|p| !p.is_empty()) else {
        return Ok(());
    };
    let Some(socket) = paths::socket_from_tmux(&tmux) else {
        return Ok(());
    };
    let paths = paths::current(&socket)?;
    let result = record(&args, &socket, &paths, pane).await;
    if let Err(error) = &result {
        let _ = super::log(&paths.state, error);
    }
    result
}
async fn record(
    args: &[String],
    socket: &Path,
    paths: &paths::Paths,
    pane: String,
) -> io::Result<()> {
    let Some((pid, agent)) = proc::PsProcessTree
        .ancestors(std::process::id())?
        .into_iter()
        .find_map(|(pid, p)| proc::agent_kind(&p.comm).map(|a| (pid, a)))
    else {
        return Ok(());
    };
    paths::prepare(paths)?;
    let format = args.first().map(String::as_str).unwrap_or("");
    if !matches!(
        format,
        "claude" | "codex" | "codex-notify" | "grok" | "opencode"
    ) {
        return Err(io::Error::other("unknown hook format"));
    }
    let raw = if format == "codex-notify" {
        args.last()
            .filter(|_| args.len() > 1)
            .cloned()
            .unwrap_or_default()
    } else {
        let mut text = String::new();
        std::io::stdin()
            .take(900 * 1024)
            .read_to_string(&mut text)?;
        text
    };
    let payload: serde_json::Value = serde_json::from_str(&raw)?;
    let (kind, agent_session_id, cwd, transcript_path) = signal::parse(format, payload.clone())?;
    let server_start = crate::tmux::hook_server_start(socket).await?;
    let signal = Signal {
        server_start,
        pane_id: pane,
        agent,
        agent_pid: pid,
        at: crate::daemon::now(),
        source: format!("hook:{format}"),
        kind,
        agent_session_id,
        cwd,
        transcript_path,
        payload,
    };
    signal.validate()?;
    let mut line = serde_json::to_vec(
        &serde_json::json!({"jsonrpc":"2.0","method":"signal","params":signal}),
    )?;
    line.push(b'\n');
    let sent = tokio::time::timeout(Duration::from_millis(100), async {
        let mut stream = client::connect(paths).await?;
        client::verify(&mut stream, socket).await?;
        stream.write_all(&line).await
    })
    .await;
    if !matches!(sent, Ok(Ok(()))) {
        spool::append(paths, &line)?;
        let _ = super::log(&paths.state, &"daemon unavailable; signal spooled");
        client::trigger_spawn(socket, paths)?;
    }
    Ok(())
}
