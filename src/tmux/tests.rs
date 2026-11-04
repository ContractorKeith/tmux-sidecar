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

#[test]
fn byte_cwd_and_bad_record_are_isolated() {
    use std::os::unix::ffi::OsStrExt;
    let record = |cwd: &[u8], pid: &[u8]| {
        let fields: [&[u8]; 8] = [b"1", b"%7", pid, cwd, b"code\xff", b"1", b"0", b"0"];
        let mut bytes = Vec::new();
        for field in fields {
            bytes.extend_from_slice(format!("{}:", field.len()).as_bytes());
            bytes.extend_from_slice(field);
        }
        bytes.push(b'\n');
        bytes
    };
    let scan = parse_scan(&record(b"/bad-\xff", b"42")).unwrap();
    assert_eq!(scan.panes[0].cwd.as_os_str().as_bytes(), b"/bad-\xff");
    assert!(scan.panes[0].current_command.contains('\u{fffd}'));
    let scan = parse_scan(&record(b"/work", b"bad-pid")).unwrap();
    assert!(scan.panes.is_empty());
    assert_eq!(scan.skipped, ["%7"]);
}
#[test]
fn only_gone_diagnostics_or_missing_socket_are_evidence() {
    let error = |s| std::io::Error::other(s);
    assert!(!gone_evidence(&error("permission denied"), true));
    assert!(!gone_evidence(&error("unknown format"), true));
    assert!(gone_evidence(
        &error("no server running on /tmp/test"),
        true
    ));
    assert!(gone_evidence(&error("error connecting to /tmp/test"), true));
    assert!(gone_evidence(&error("transient"), false));
}

#[test]
fn server_exit_requires_the_second_probe_to_agree() {
    let gone = || std::io::Error::other("no server running");
    assert!(!confirm_gone(&gone(), || true, || Ok(PaneScan::default())));
    assert!(!confirm_gone(
        &gone(),
        || true,
        || Err(std::io::Error::other("permission denied"))
    ));
    assert!(confirm_gone(&gone(), || true, || Err(gone())));
    assert!(!confirm_gone(
        &std::io::Error::other("bad format"),
        || true,
        || panic!("must not probe a transient error")
    ));
}
