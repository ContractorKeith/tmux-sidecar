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
const HOOK_BUDGET: Duration = Duration::from_millis(180);
fn live_budget(elapsed: Duration) -> Duration {
    HOOK_BUDGET
        .saturating_sub(Duration::from_millis(10))
        .saturating_sub(elapsed)
        .min(Duration::from_millis(100))
}
async fn deliver(
    socket: &Path,
    paths: &paths::Paths,
    line: &[u8],
    budget: Duration,
) -> Result<io::Result<()>, tokio::time::error::Elapsed> {
    tokio::time::timeout(budget, async {
        let mut stream = client::connect(paths).await?;
        client::verify(&mut stream, socket).await?;
        stream.write_all(line).await
    })
    .await
}
pub async fn run(args: Vec<String>) -> io::Result<()> {
    let format = args.first().map(String::as_str).unwrap_or("");
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
    let raw = if format == "codex-notify" {
        Ok((notify_payload(&args), false))
    } else {
        input::read_stdin()
    };
    let Some(pane) = std::env::var("TMUX_PANE").ok().filter(|p| !p.is_empty()) else {
        return Ok(());
    };
    let Some(socket) = paths::socket_from_tmux(&tmux) else {
        return Ok(());
    };
    let paths = paths::current(&socket)?;
    let started = Instant::now();
    let result = match raw {
        Ok((raw, expired)) => {
            if expired {
                paths::prepare(&paths)?;
                super::log(
                    &paths.state,
                    &format!("hook stdin read budget expired after {} bytes", raw.len()),
                )?;
                if raw.is_empty() {
                    return Ok(());
                }
                if serde_json::from_slice::<serde_json::Value>(&raw).is_err() {
                    super::log(
                        &paths.state,
                        &format!("partial payload {} bytes", raw.len()),
                    )?;
                    return Ok(());
                }
            }
            record(&raw, format, pane, &socket, &paths, started).await
        }
        Err(error) => Err(error),
    };
    if let Err(error) = &result {
        let _ = paths::prepare(&paths);
        let _ = super::log(&paths.state, error);
    }
    if result.is_ok() && started.elapsed() >= HOOK_BUDGET {
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
    let chain = proc::hook_ancestors()?;
    let Some((pid, agent)) = chain
        .iter()
        .find_map(|(pid, p)| proc::agent_kind(&p.comm).map(|a| (*pid, a)))
    else {
        return Err(io::Error::other("hook dropped: no agent ancestor"));
    };
    paths::prepare(paths)?;
    let payload: serde_json::Value = serde_json::from_slice(raw)?;
    let parsed = signal::parse(format, &payload)?;
    let mut signal = Signal {
        server_key: Some(paths::server_key(socket)?),
        pane_verified: false,
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
        orphaned: false,
        parent_agent_pid: None,
        payload: input::compact(payload)?,
    };
    super::attribution::resolve(&mut signal, &chain, &paths.runtime, socket).await;
    if signal.orphaned {
        let _ = super::log(
            &paths.state,
            &"hook ancestry does not reach pane; route to an unambiguous owner or drop",
        );
    }
    signal.validate_frame()?;
    let frame = |signal: &Signal| -> io::Result<Vec<u8>> {
        let mut line = serde_json::to_vec(
            &serde_json::json!({"jsonrpc":"2.0","method":"signal","params":signal}),
        )?;
        line.push(b'\n');
        Ok(line)
    };
    // Exactly one O_APPEND write precedes live delivery and daemon spawning.
    let spooled = spool::append(paths, &frame(&signal)?);
    if let Err(error) = &spooled {
        let _ = super::log(
            &paths.state,
            &format!("spool append failed; attempting live delivery: {error}"),
        );
    }
    if signal.server_start == 0
        && let Ok(start) = crate::tmux::hook_server_start(socket).await
    {
        signal.server_start = start;
    }
    let remaining = live_budget(started.elapsed());
    if !remaining.is_zero() && signal.server_start != 0 {
        let line = frame(&signal)?;
        let sent = deliver(socket, paths, &line, remaining).await;
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
    spooled.map_err(|e| {
        io::Error::other(format!("signal lost: spool and live delivery failed: {e}"))
    })?;
    let _ = super::log(
        &paths.state,
        &"daemon unavailable; durable signal awaits replay",
    );
    let _ = client::trigger_spawn(socket, paths);
    Ok(())
}

fn notify_payload(args: &[String]) -> Vec<u8> {
    args.last()
        .filter(|_| args.len() > 1)
        .cloned()
        .unwrap_or_default()
        .into_bytes()
}

#[cfg(test)]
#[path = "delivery_tests.rs"]
mod tests;
