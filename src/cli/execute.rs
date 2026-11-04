use super::*;
use crate::{client, daemon, paths};
use serde_json::json;
use std::{io, time::Duration};
type Failure = (u8, String);
fn response_error(error: io::Error) -> Failure {
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

pub async fn execute(cli: Cli) -> Result<(), Failure> {
    if !matches!(cli.command, Command::List { .. } | Command::Daemon { .. }) {
        let (verb, milestone) = cli.command.pending();
        return Err((2, format!("{verb} is not implemented yet (M{milestone})")));
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
    let (method, params, spawn, json_output) = match cli.command {
        Command::List { group, state, json } => {
            ("list", json!({"group":group,"state":state}), true, json)
        }
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
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while paths.runtime.join("sock").exists() {
            if tokio::time::Instant::now() >= deadline {
                return Err((6, "daemon did not stop within 2 s".into()));
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    } else if json_output {
        println!(
            "{}",
            serde_json::to_string(&result).map_err(|e| (1, e.to_string()))?
        );
    } else if method == "list" {
        println!("RUN\tPANE\tAGENT\tSTATE\tPID\tCWD");
        if let Some(runs) = result.as_array() {
            for run in runs {
                let text = |key: &str| run[key].as_str().unwrap_or("");
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{}",
                    text("id"),
                    text("pane_id"),
                    text("agent"),
                    text("state"),
                    run["agent_pid"],
                    text("cwd")
                );
            }
        }
    } else {
        println!(
            "running (pid {}, server {})",
            result["pid"],
            result["server_key"].as_str().unwrap_or("")
        );
    }
    Ok(())
}
