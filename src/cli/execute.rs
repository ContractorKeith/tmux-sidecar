use super::*;
use crate::{client, daemon, paths};
use serde_json::json;
use std::{io, time::Duration};
pub(super) type Failure = (u8, String);
pub(super) fn response_error(error: io::Error) -> Failure {
    if let Some(rpc) = error
        .get_ref()
        .and_then(|e| e.downcast_ref::<crate::rpc::RpcError>())
    {
        let code = match rpc.code {
            crate::rpc::INVALID_PARAMS | crate::rpc::METHOD_NOT_FOUND | crate::rpc::USAGE => 2,
            crate::rpc::REFUSED => 3,
            crate::rpc::TIMEOUT => 4,
            crate::rpc::NOT_FOUND => 5,
            crate::rpc::DAEMON_UNREACHABLE => 6,
            _ => 1,
        };
        return (code, error.to_string());
    }
    unreachable(error)
}
fn unreachable(error: io::Error) -> Failure {
    (6, format!("daemon unreachable: {error}"))
}

pub(crate) fn output_error(error: io::Error) -> Failure {
    (
        if error.kind() == io::ErrorKind::BrokenPipe {
            0
        } else {
            1
        },
        error.to_string(),
    )
}
pub async fn execute(cli: Cli) -> Result<(), Failure> {
    match execute_inner(cli).await {
        Err((0, _)) => Ok(()),
        result => result,
    }
}
async fn execute_inner(mut cli: Cli) -> Result<(), Failure> {
    if let Command::Hook { agent, payload } = &cli.command {
        let mut args = vec![agent.to_owned()];
        args.extend(payload.iter().cloned());
        crate::hook::entry(args).await;
        return Ok(());
    }
    if let Command::Hooks { command } = &cli.command {
        return super::hooks::run(command, cli.quiet);
    }
    if let Command::Mcp { allow_send } = cli.command {
        let socket = client::select_socket(cli.tmux_socket.as_deref()).map_err(unreachable)?;
        let paths = paths::current(&socket).map_err(unreachable)?;
        return crate::mcp::run(socket, paths, allow_send)
            .await
            .map_err(|e| (1, e.to_string()));
    }
    super::input::prepare(&mut cli)?;
    if let Command::Doctor { json, agent } = cli.command {
        return crate::doctor::run(cli.tmux_socket.as_deref(), json, cli.quiet, &agent).await;
    }
    let socket = client::select_socket(cli.tmux_socket.as_deref()).map_err(unreachable)?;
    let paths = paths::current(&socket).map_err(unreachable)?;
    if let Command::Daemon {
        command: Daemon::Run,
    } = cli.command
    {
        return daemon::run(socket, paths)
            .await
            .map_err(|e| (1, e.to_string()));
    }
    if matches!(cli.command, Command::Ui) {
        return crate::ui::run(socket, paths)
            .await
            .map_err(|e| (1, e.to_string()));
    }
    if let Command::Watch { json } = cli.command {
        return super::watch::run(&socket, &paths, json, cli.quiet).await;
    }
    let (method, params, spawn, json_output) = match cli.command {
        Command::Attention { json } => ("attention", json!({}), true, json),
        Command::List {
            group,
            state,
            json,
            all,
            limit,
        } => (
            "list",
            json!({"group":group,"state":state,"all":all,"limit":limit}),
            true,
            json,
        ),
        Command::Wait {
            sel,
            until,
            timeout,
            json,
        } => {
            let until = match until {
                WaitState::Waiting => "waiting",
                WaitState::Blocked => "blocked",
                WaitState::Done => "done",
            };
            (
                "wait",
                json!({"sel":sel.to_string(),"until":until,"timeout":timeout}),
                true,
                json,
            )
        }
        Command::Send { sel, text, force } => (
            "send",
            json!({"sel":sel.to_string(),"text":text,"force":force}),
            true,
            false,
        ),
        Command::Ack { sel } => (
            "ack",
            json!({"selectors":sel.iter().map(ToString::to_string).collect::<Vec<_>>()}),
            true,
            false,
        ),
        Command::History { sel, limit, json } => (
            "history",
            json!({"sel":sel.to_string(),"limit":limit}),
            true,
            json,
        ),
        Command::Name { sel, name, .. } => (
            "name",
            json!({"sel":sel.to_string(),"name":name}),
            true,
            false,
        ),
        Command::Group { command } => match command {
            Group::Ls { json } => ("group.ls", json!({}), true, json),
            Group::Add { group, sel } => (
                "group.add",
                json!({"group":group,"selectors":sel.iter().map(ToString::to_string).collect::<Vec<_>>()}),
                true,
                false,
            ),
            Group::Rm { group, sel } => (
                "group.rm",
                json!({"group":group,"selectors":sel.iter().map(ToString::to_string).collect::<Vec<_>>()}),
                true,
                false,
            ),
        },
        Command::Daemon {
            command: Daemon::Status { json },
        } => ("daemon.status", json!({}), false, json),
        Command::Daemon {
            command: Daemon::Stop,
        } => ("daemon.stop", json!({}), false, false),
        _ => unreachable!(),
    };
    let result = client::request(&socket, &paths, method, params, spawn)
        .await
        .map_err(response_error)?;
    if method == "daemon.stop" {
        let pid = result["pid"]
            .as_i64()
            .and_then(|pid| i32::try_from(pid).ok())
            .filter(|pid| *pid > 0)
            .ok_or_else(|| (6, "daemon.stop response did not identify its pid".into()))?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while crate::proc::running(pid).map_err(unreachable)? || paths.runtime.join("sock").exists()
        {
            if tokio::time::Instant::now() >= deadline {
                return Err((
                    6,
                    format!("waiting for daemon pid {pid} to exit and socket cleanup exceeded 5 s"),
                ));
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    } else if !cli.quiet {
        super::output::print_read(&socket, &paths, method, &result, json_output)
            .await
            .map_err(output_error)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "failure_tests.rs"]
mod tests;
