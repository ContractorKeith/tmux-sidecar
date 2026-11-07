use crate::seams::PaneSnapshot;
use std::{ffi::OsStr, io, os::unix::ffi::OsStrExt, path::PathBuf};
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PaneScan {
    pub panes: Vec<PaneSnapshot>,
    pub skipped: Vec<String>,
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid tmux pane framing")
}
fn field<'a>(input: &mut &'a [u8]) -> io::Result<&'a [u8]> {
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
    let value = rest.get(..len).ok_or_else(invalid)?;
    *input = &rest[len..];
    Ok(value)
}
pub fn parse(input: &[u8]) -> io::Result<Vec<PaneSnapshot>> {
    Ok(parse_scan(input)?.panes)
}
pub fn parse_scan(mut input: &[u8]) -> io::Result<PaneScan> {
    let mut scan = PaneScan::default();
    while !input.is_empty() {
        let fields: Vec<_> = (0..9)
            .map(|_| field(&mut input))
            .collect::<io::Result<_>>()?;
        input = input.strip_prefix(b"\n").ok_or_else(invalid)?;
        let text: Vec<_> = fields.iter().map(|f| String::from_utf8_lossy(f)).collect();
        let pane_id = text[1].to_string();
        // Without a usable id we cannot protect the previous run from this incomplete tick.
        if !matches!(pane_id.parse(), Ok(crate::model::RunSelector::Pane(_))) {
            return Err(invalid());
        }
        // Dead panes are authoritative exits even if their old pid was reused.
        if text[8] == "1" {
            continue;
        }
        if text[8] != "0" {
            scan.skipped.push(pane_id);
            continue;
        }
        let record = || -> Option<PaneSnapshot> {
            let cwd = PathBuf::from(OsStr::from_bytes(fields[3]));
            if !cwd.is_absolute() {
                return None;
            }
            let flag = |v: &str| match v {
                "0" => Some(false),
                "1" => Some(true),
                _ => None,
            };
            Some(PaneSnapshot {
                server_start: text[0].parse().ok()?,
                pane_id: pane_id.clone(),
                pane_pid: text[2].parse().ok()?,
                cwd,
                current_command: text[4].to_string(),
                pane_active: flag(&text[5])?,
                window_active: flag(&text[6])?,
                session_attached: text[7].parse::<u32>().ok()? > 0,
            })
        };
        if let Some(pane) = record() {
            scan.panes.push(pane);
        } else {
            scan.skipped.push(pane_id);
        }
    }
    Ok(scan)
}
