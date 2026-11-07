use super::*;
#[test]
fn precedence_unknown_exact_matching_reordering_duplicates_and_idle() {
    let mut current = run(AgentKind::Claude);
    current.state = Waiting;
    for (event, detail) in [
        ("StopCancelled", None),
        ("StopFailure-extra", None),
        ("Notification", Some("permission_prompt_extra")),
    ] {
        assert_eq!(
            transition(
                &current,
                Evidence::Hook {
                    event,
                    detail,
                    at: 101,
                    last_signal_at: Some(current.state_since)
                },
                101
            ),
            (Waiting, false)
        );
    }
    assert_eq!(
        transition(
            &current,
            Evidence::Hook {
                event: "PreToolUse",
                detail: None,
                at: 99,
                last_signal_at: Some(current.state_since)
            },
            101
        ),
        (Waiting, false)
    );
    assert_eq!(
        transition(
            &current,
            Evidence::Hook {
                event: "PreToolUse",
                detail: None,
                at: 99,
                last_signal_at: None
            },
            101
        ),
        (Working, true)
    );
    assert_eq!(
        transition(
            &current,
            Evidence::Hook {
                event: "Stop",
                detail: None,
                at: 101,
                last_signal_at: Some(current.state_since)
            },
            101
        ),
        (Waiting, false)
    );
    assert_eq!(
        transition(
            &current,
            Evidence::Transcript {
                state: Working,
                has_hooks: true,
                at: 101
            },
            101
        ),
        (Waiting, false)
    );
    assert_eq!(
        transition(
            &current,
            Evidence::Transcript {
                state: Working,
                has_hooks: false,
                at: 101
            },
            101
        ),
        (Working, true)
    );
    assert_eq!(
        transition(&current, Evidence::Tick { idle_after_ms: 100 }, 201),
        (Waiting, false)
    );
    current.read_at = Some(100);
    assert_eq!(
        transition(&current, Evidence::Tick { idle_after_ms: 100 }, 200),
        (Waiting, false)
    );
    assert_eq!(
        transition(&current, Evidence::Tick { idle_after_ms: 100 }, 201),
        (Idle, true)
    );
    assert_eq!(transition(&current, Evidence::Exit, 201), (Done, true));
    current.ended_at = Some(150);
    assert_eq!(
        transition(
            &current,
            Evidence::Hook {
                event: "PreToolUse",
                detail: None,
                at: 201,
                last_signal_at: Some(current.state_since)
            },
            201
        ),
        (Done, true)
    );
}

#[test]
fn rereading_completed_transcript_does_not_undo_clock_idle() {
    let mut current = run(AgentKind::Claude);
    current.state_source = "clock".into();
    assert_eq!(
        transition(
            &current,
            Evidence::Transcript {
                state: Waiting,
                has_hooks: false,
                at: 99
            },
            200
        ),
        (Idle, false)
    );
    assert_eq!(
        transition(
            &current,
            Evidence::Transcript {
                state: Waiting,
                has_hooks: false,
                at: 101
            },
            200
        ),
        (Waiting, true)
    );
}
