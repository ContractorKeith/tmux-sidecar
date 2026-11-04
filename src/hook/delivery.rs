use super::{
    context, input,
    signal::{self, Signal},
    spool,
};
use crate::{client, paths, proc};
use std::{
    io,
    path::Path,
    time::{Duration, Instant},
};
use tokio::io::AsyncWriteExt;
pub async fn run(args: Vec<String>) -> io::Result<()> {
    let started = Instant::now();
    let format = args.first().map(String::as_str).unwrap_or("");
    // Drain piped input for every format, even on no-op paths. Notify may inherit a tty.
    use std::io::IsTerminal;
    let drained = if std::io::stdin().is_terminal() {
        Ok(Vec::new())
    } else {
        input::read(&mut std::io::stdin())
    };
    let raw = if format == "codex-notify" {
        drained.map(|_| args.get(1).cloned().unwrap_or_default().into_bytes())
    } else {
        drained
    };
    if !matches!(
        format,
        "claude" | "codex" | "codex-notify" | "grok" | "opencode"
    ) {
        let _ = super::unknown_format();
        return Err(io::Error::other("unknown hook format"));
    }
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
    let result = match raw {
        Ok(raw) => record(&raw, format, pane, &socket, &paths, started).await,
        Err(error) => Err(error),
    };
    if let Err(error) = &result {
        let _ = paths::prepare(&paths);
        let _ = super::log(&paths.state, error);
    }
    if started.elapsed() >= Duration::from_millis(180) {
        let _ = super::log(&paths.state, &"budget exceeded");
    }
    result
}
async fn record(
    raw: &[u8],
    format: &str,
    pane: String,
    socket: &Path,
    paths: &paths::Paths,
    started: Instant,
) -> io::Result<()> {
    let Some((pid, agent)) = proc::hook_ancestors()?
        .into_iter()
        .find_map(|(pid, p)| proc::agent_kind(&p.comm).map(|a| (pid, a)))
    else {
        return Ok(());
    };
    paths::prepare(paths)?;
    let payload: serde_json::Value = serde_json::from_slice(raw)?;
    let parsed = signal::parse(format, &payload)?;
    let mut signal = Signal {
        server_start: context::read(&paths.runtime, socket).unwrap_or(0),
        pane_id: pane,
        agent,
        agent_pid: pid,
        at: crate::daemon::now(),
        nonce: signal::nonce()?,
        source: format!("hook:{format}"),
        event: parsed.event,
        detail: parsed.detail,
        agent_session_id: parsed.session_id,
        cwd: parsed.cwd,
        transcript_path: parsed.transcript_path,
        payload: input::compact(payload)?,
    };
    signal.validate_frame()?;
    let frame = |signal: &Signal| -> io::Result<Vec<u8>> {
        let mut line = serde_json::to_vec(
            &serde_json::json!({"jsonrpc":"2.0","method":"signal","params":signal}),
        )?;
        line.push(b'\n');
        Ok(line)
    };
    // Exactly one O_APPEND write precedes slow tmux, socket, and spawn operations.
    spool::append(paths, &frame(&signal)?)
        .map_err(|e| io::Error::other(format!("signal lost: spool append: {e}")))?;
    if signal.server_start == 0
        && let Ok(start) = crate::tmux::hook_server_start(socket).await
    {
        signal.server_start = start;
    }
    let remaining = Duration::from_millis(170).saturating_sub(started.elapsed());
    if !remaining.is_zero() && signal.server_start != 0 {
        let line = frame(&signal)?;
        let sent = tokio::time::timeout(remaining.min(Duration::from_millis(100)), async {
            let mut stream = client::connect(paths).await?;
            client::verify(&mut stream, socket).await?;
            stream.write_all(&line).await
        })
        .await;
        if sent.is_err() {
            let _ = super::log(
                &paths.state,
                &"budget exceeded during live delivery; durable signal retained",
            );
        }
        if matches!(sent, Ok(Ok(()))) {
            return Ok(());
        }
    }
    let _ = super::log(
        &paths.state,
        &"daemon unavailable; durable signal awaits replay",
    );
    let _ = client::trigger_spawn(socket, paths);
    Ok(())
}
