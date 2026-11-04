//! M0: pane metadata adapter; mutations arrive in M4/M5.
use crate::seams::{PaneSnapshot, Tmux};
use std::{io, path::PathBuf, process::Command};

// Length framing preserves tabs/newlines in cwd and command (session-board's idiom).
const FORMAT: &str = concat!(
    "#{n:start_time}:#{start_time}",
    "#{n:pane_id}:#{pane_id}",
    "#{n:pane_pid}:#{pane_pid}",
    "#{n:pane_current_path}:#{pane_current_path}",
    "#{n:pane_current_command}:#{pane_current_command}",
    "#{n:pane_active}:#{pane_active}",
    "#{n:window_active}:#{window_active}",
    "#{n:session_attached}:#{session_attached}",
);

pub enum Socket {
    Path(PathBuf),
    Name(String),
}
pub struct RealTmux {
    pub socket: Socket,
}
impl RealTmux {
    pub fn command(&self) -> Command {
        let mut command = Command::new("tmux");
        match &self.socket {
            Socket::Path(path) => {
                command.arg("-S").arg(path);
            }
            Socket::Name(name) => {
                command.arg("-L").arg(name);
            }
        }
        command
    }
}
impl Tmux for RealTmux {
    fn list_panes(&self) -> io::Result<Vec<PaneSnapshot>> {
        let output = self
            .command()
            .args(["list-panes", "-a", "-F", FORMAT])
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(
                String::from_utf8_lossy(&output.stderr).into_owned(),
            ));
        }
        parse(&output.stdout)
    }
    fn paste_text(&self, _: &str, _: &str) -> io::Result<()> {
        Err(unsupported("M4"))
    }
    fn send_enter(&self, _: &str) -> io::Result<()> {
        Err(unsupported("M4"))
    }
    fn display_message(&self, _: &str, _: &str) -> io::Result<()> {
        Err(unsupported("M5"))
    }
}
fn unsupported(milestone: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        format!("not implemented yet ({milestone})"),
    )
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid tmux pane record")
}
fn field<'a>(input: &mut &'a [u8]) -> io::Result<&'a str> {
    let colon = input.iter().position(|b| *b == b':').ok_or_else(invalid)?;
    let digits = &input[..colon];
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return Err(invalid());
    }
    let len = std::str::from_utf8(digits)
        .map_err(|_| invalid())?
        .parse::<usize>()
        .map_err(|_| invalid())?;
    let rest = &input[colon + 1..];
    let value = std::str::from_utf8(rest.get(..len).ok_or_else(invalid)?).map_err(|_| invalid())?;
    *input = &rest[len..];
    Ok(value)
}
pub fn parse(mut input: &[u8]) -> io::Result<Vec<PaneSnapshot>> {
    let mut panes = Vec::new();
    while !input.is_empty() {
        let f: Vec<_> = (0..8)
            .map(|_| field(&mut input))
            .collect::<io::Result<_>>()?;
        input = input.strip_prefix(b"\n").ok_or_else(invalid)?;
        if f[3].is_empty() {
            continue;
        }
        if !matches!(f[1].parse(), Ok(crate::model::RunSelector::Pane(_)))
            || !std::path::Path::new(f[3]).is_absolute()
        {
            return Err(invalid());
        }
        let flag = |value: &str| match value {
            "0" => Ok(false),
            "1" => Ok(true),
            _ => Err(invalid()),
        };
        panes.push(PaneSnapshot {
            server_start: f[0].parse().map_err(|_| invalid())?,
            pane_id: f[1].into(),
            pane_pid: f[2].parse().map_err(|_| invalid())?,
            cwd: f[3].into(),
            current_command: f[4].into(),
            pane_active: flag(f[5])?,
            window_active: flag(f[6])?,
            session_attached: f[7].parse::<u32>().map_err(|_| invalid())? > 0,
        });
    }
    Ok(panes)
}
/// Used only when neither an explicit socket nor TMUX selects a server.
pub fn default_socket() -> io::Result<PathBuf> {
    let output = Command::new("tmux")
        .args(["display", "-p", "#{socket_path}"])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::NotConnected,
            "no tmux server; start tmux or select one with --tmux-socket",
        ));
    }
    let path = String::from_utf8(output.stdout).map_err(io::Error::other)?;
    if path.trim().is_empty() {
        return Err(io::Error::other("tmux returned an empty socket path"));
    }
    Ok(PathBuf::from(path.trim_end()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_cwd_only_skips_its_pane() {
        let record = |cwd: &str| {
            ["1234", "%7", "42", cwd, "codex", "1", "0", "0"]
                .iter()
                .map(|s| format!("{}:{s}", s.len()))
                .collect::<String>()
                + "\n"
        };
        let good = record("/project");
        let missing = record("");
        assert_eq!(
            parse((missing.clone() + &good).as_bytes()).unwrap(),
            parse(good.as_bytes()).unwrap()
        );
        assert_eq!(
            parse((good.clone() + &missing).as_bytes()).unwrap(),
            parse(good.as_bytes()).unwrap()
        );
    }
    #[test]
    fn canned_panes_and_malformed_records() {
        let fields = [
            "1234",
            "%7",
            "42",
            "/project é\tand\nnewline",
            "codex",
            "1",
            "0",
            "2",
        ];
        let record = fields
            .iter()
            .map(|s| format!("{}:{s}", s.len()))
            .collect::<String>()
            + "\n";
        let panes = parse(record.as_bytes()).unwrap();
        assert_eq!(
            panes[0],
            PaneSnapshot {
                server_start: 1234,
                pane_id: "%7".into(),
                pane_pid: 42,
                cwd: fields[3].into(),
                current_command: "codex".into(),
                pane_active: true,
                window_active: false,
                session_attached: true
            }
        );
        assert_eq!(parse(b"").unwrap(), vec![]);
        for bad in ["1:x", "99999999999999999999999999:x", "x:bad"] {
            assert!(parse(bad.as_bytes()).is_err());
        }
        assert!(parse((record + "bad").as_bytes()).is_err());
    }
}
