//! Bounded tmux mutations, always against the explicitly selected server.
use std::{io, path::Path, time::Duration};
pub(crate) async fn output(socket: &Path, args: &[&str]) -> io::Result<String> {
    let output = tokio::time::timeout(
        Duration::from_secs(1),
        tokio::process::Command::new("tmux")
            .arg("-S")
            .arg(socket)
            .args(args)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(io::Error::other)??;
    if !output.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    String::from_utf8(output.stdout).map_err(io::Error::other)
}
pub async fn focus(socket: &Path, pane: &str, server_start: u64) -> io::Result<()> {
    if !matches!(
        pane.parse::<crate::model::RunSelector>(),
        Ok(crate::model::RunSelector::Pane(_))
    ) {
        return Err(io::Error::other("invalid pane id"));
    }
    if output(socket, &["list-clients", "-F", "#{client_name}"])
        .await?
        .trim()
        .is_empty()
    {
        return Err(io::Error::other("run the ui inside tmux to switch panes"));
    }
    let context = output(
        socket,
        &[
            "display-message",
            "-p",
            "-t",
            pane,
            "#{start_time}\t#{session_id}\t#{window_id}",
        ],
    )
    .await?;
    let fields: Vec<_> = context.trim_end().split('\t').collect();
    if fields.len() != 3 || fields[0].parse::<u64>().ok() != Some(server_start) {
        return Err(io::Error::other("tmux server restarted; run is stale"));
    }
    output(socket, &["switch-client", "-t", fields[1]]).await?;
    output(socket, &["select-window", "-t", fields[2]]).await?;
    output(socket, &["select-pane", "-t", pane]).await?;
    Ok(())
}
pub(crate) async fn waiting(
    socket: &Path,
    pane: &str,
    server_start: u64,
    message: &str,
) -> io::Result<()> {
    let clients = output(
        socket,
        &["list-clients", "-F", "#{client_name}\t#{session_id}"],
    )
    .await?;
    let session = output(
        socket,
        &[
            "display-message",
            "-p",
            "-t",
            pane,
            "#{start_time}\t#{session_id}",
        ],
    )
    .await?;
    let Some((start, session)) = session.trim().split_once('\t') else {
        return Err(io::Error::other("pane no longer exists"));
    };
    if start.parse::<u64>().ok() != Some(server_start) {
        return Err(io::Error::other("tmux server restarted"));
    }
    for client in clients.lines() {
        if let Some((name, attached)) = client.split_once('\t')
            && attached == session.trim()
        {
            // display-message expands tmux formats even with argv. Escape # so the
            // message is literal text, including any future user-provided names.
            let literal = message.replace('#', "##");
            output(socket, &["display-message", "-c", name, "--", &literal]).await?;
        }
    }
    Ok(())
}
