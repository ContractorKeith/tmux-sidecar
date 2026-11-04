use super::*;
use crate::seams::{
    Process, ProcessTree, Tmux,
    fakes::{FakeProcessTree, FakeTmux},
};

#[test]
fn detects_grandchild_newest_and_ends_missing_runs() {
    let tmux = FakeTmux::default();
    tmux.panes.borrow_mut().push(PaneSnapshot {
        server_start: 1,
        pane_id: "%0".into(),
        pane_pid: 10,
        cwd: "/work".into(),
        current_command: "sh".into(),
        pane_active: false,
        window_active: false,
        session_attached: false,
    });
    let tree = FakeProcessTree::default();
    tree.processes.borrow_mut().extend([
        (
            10,
            Process {
                ppid: 1,
                comm: "sh".into(),
            },
        ),
        (
            11,
            Process {
                ppid: 10,
                comm: "sh".into(),
            },
        ),
    ]);
    let panes = tmux.list_panes().unwrap();
    assert!(
        detect(&panes, &tree.snapshot().unwrap(), &[], 1)
            .start
            .is_empty()
    );
    tree.processes.borrow_mut().insert(
        12,
        Process {
            ppid: 11,
            comm: "claude".into(),
        },
    );
    let mut run = detect(&panes, &tree.snapshot().unwrap(), &[], 2)
        .start
        .remove(0);
    assert_eq!(run.agent_pid, 12);
    assert_eq!(run.state, AgentState::Waiting);
    run.id = "r1".into();
    assert!(
        detect(&panes, &tree.snapshot().unwrap(), &[run.clone()], 3)
            .start
            .is_empty()
    );
    tree.processes.borrow_mut().insert(
        13,
        Process {
            ppid: 11,
            comm: "codex".into(),
        },
    );
    let changes = detect(&panes, &tree.snapshot().unwrap(), &[run.clone()], 4);
    assert_eq!(changes.start[0].agent_pid, 13);
    assert_eq!(changes.end, ["r1"]);
    tree.processes.borrow_mut().remove(&12);
    tree.processes.borrow_mut().remove(&13);
    assert_eq!(
        detect(&panes, &tree.snapshot().unwrap(), &[run.clone()], 5).end,
        ["r1"]
    );
    assert_eq!(
        detect(&[], &tree.snapshot().unwrap(), &[run], 6).end,
        ["r1"]
    );
}

#[test]
fn ignores_nested_helpers_and_handles_missing_or_cyclic_ancestors() {
    let pane = PaneSnapshot {
        server_start: 1,
        pane_id: "%0".into(),
        pane_pid: 10,
        cwd: "/work".into(),
        current_command: "sh".into(),
        pane_active: false,
        window_active: false,
        session_attached: false,
    };
    let mut processes: ProcessSnapshot = [
        (
            10,
            Process {
                ppid: 1,
                comm: "sh".into(),
            },
        ),
        (
            20,
            Process {
                ppid: 10,
                comm: "claude".into(),
            },
        ),
        (
            30,
            Process {
                ppid: 20,
                comm: "codex".into(),
            },
        ),
        (
            40,
            Process {
                ppid: 41,
                comm: "grok".into(),
            },
        ),
        (
            41,
            Process {
                ppid: 40,
                comm: "sh".into(),
            },
        ),
        (
            50,
            Process {
                ppid: 99,
                comm: "opencode".into(),
            },
        ),
    ]
    .into();
    assert_eq!(
        detect(std::slice::from_ref(&pane), &processes, &[], 1).start[0].agent_pid,
        20
    );
    // Independent agents at different depths still use the specified newest-pid rule.
    processes.insert(
        60,
        Process {
            ppid: 10,
            comm: "sh".into(),
        },
    );
    processes.insert(
        70,
        Process {
            ppid: 60,
            comm: "grok".into(),
        },
    );
    assert_eq!(detect(&[pane], &processes, &[], 2).start[0].agent_pid, 70);
}
