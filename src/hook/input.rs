//! Read within the hook deadline and retain a bounded payload preview.
use serde_json::Value;
use std::io::{self, Read};
const INPUT_LIMIT: usize = 64 * 1024 * 1024;
/// Poll a nonblocking stdin only within the hook's input budget.
pub fn read_stdin(deadline: std::time::Instant) -> io::Result<(Vec<u8>, bool)> {
    use std::{io::IsTerminal, os::fd::AsRawFd};
    let stdin = io::stdin();
    if stdin.is_terminal() {
        return Ok((Vec::new(), false));
    }
    let fd = stdin.as_raw_fd();
    // SAFETY: stdin is live; fcntl only queries/updates descriptor flags.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: same live descriptor, flags came from F_GETFL.
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    struct Restore(i32, i32);
    impl Drop for Restore {
        fn drop(&mut self) {
            // SAFETY: stdin remains live throughout this scope.
            unsafe {
                libc::fcntl(self.0, libc::F_SETFL, self.1);
            }
        }
    }
    let _restore = Restore(fd, flags);
    let mut bytes = Vec::new();
    let mut chunk = [0; 64 * 1024];
    while std::time::Instant::now() < deadline {
        match stdin.lock().read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if bytes.len() + n <= INPUT_LIMIT {
                    bytes.extend_from_slice(&chunk[..n]);
                } else {
                    return Err(io::Error::other("hook input exceeds 64 MiB"));
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                let mut poll = libc::pollfd {
                    fd,
                    events: libc::POLLIN,
                    revents: 0,
                };
                let ms = deadline
                    .saturating_duration_since(std::time::Instant::now())
                    .as_millis()
                    .clamp(1, 60) as i32;
                // SAFETY: poll points at one writable pollfd; timeout is bounded.
                unsafe {
                    libc::poll(&mut poll, 1, ms);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    let expired = std::time::Instant::now() >= deadline;
    Ok((bytes, expired))
}
fn prefix(text: &str, limit: usize) -> &str {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
fn trim(value: &mut Value, budget: &mut usize) -> bool {
    if *budget == 0 {
        *value = Value::String("…truncated".into());
        return true;
    }
    match value {
        Value::String(text) => {
            let limit = (*budget).min(4096);
            let changed = text.len() > limit;
            if changed {
                *text = format!("{}…truncated", prefix(text, limit));
            }
            *budget = budget.saturating_sub(text.len());
            changed
        }
        Value::Array(items) => {
            let mut changed = items.len() > 64;
            items.truncate(64);
            for item in items.iter_mut() {
                changed |= trim(item, budget);
            }
            if changed {
                items.push(Value::String("…truncated".into()));
            }
            changed
        }
        Value::Object(object) => {
            let mut count = 0;
            let mut changed = false;
            object.retain(|key, value| {
                count += 1;
                if count > 64 || key.len() > 4096 || *budget == 0 {
                    changed = true;
                    return false;
                }
                *budget = budget.saturating_sub(key.len() + 8);
                changed |= trim(value, budget);
                true
            });
            changed
        }
        _ => {
            *budget = budget.saturating_sub(16);
            false
        }
    }
}
pub fn compact(mut payload: Value) -> io::Result<Value> {
    let changed = trim(&mut payload, &mut (48 * 1024));
    if changed && let Some(object) = payload.as_object_mut() {
        object.insert("_tmux_sidecar_truncated".into(), true.into());
    }
    let serialized = serde_json::to_string(&payload)?;
    if serialized.len() >= 256 * 1024 {
        return Ok(
            serde_json::json!({"_tmux_sidecar_truncated":true,"preview":format!("{}…truncated",prefix(&serialized,32*1024))}),
        );
    }
    Ok(payload)
}
