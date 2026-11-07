//! Read to EOF while input progresses; bound silence and the retained payload.
use serde_json::Value;
use std::io::{self, Read};
const INPUT_LIMIT: usize = 64 * 1024 * 1024;
const INPUT_IDLE: std::time::Duration = std::time::Duration::from_secs(1);
/// Give up only after a second without input progress; EOF and the size cap end reading.
pub fn read_stdin() -> io::Result<(Vec<u8>, bool)> {
    use std::os::fd::AsRawFd;
    let mut stdin = io::stdin();
    let fd = stdin.as_raw_fd();
    read_fd(&mut stdin, fd, std::time::Instant::now() + INPUT_IDLE)
}
fn read_fd(
    reader: &mut impl Read,
    fd: i32,
    mut deadline: std::time::Instant,
) -> io::Result<(Vec<u8>, bool)> {
    // SAFETY: the caller keeps this descriptor live; fcntl only queries/updates descriptor flags.
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
            // SAFETY: the caller keeps the descriptor live throughout this scope.
            unsafe {
                libc::fcntl(self.0, libc::F_SETFL, self.1);
            }
        }
    }
    let _restore = Restore(fd, flags);
    let mut bytes = Vec::new();
    let mut chunk = [0; 64 * 1024];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => return Ok((bytes, false)),
            Ok(n) => {
                if bytes.len() + n <= INPUT_LIMIT {
                    bytes.extend_from_slice(&chunk[..n]);
                    deadline = std::time::Instant::now() + INPUT_IDLE;
                } else {
                    return Err(io::Error::other("hook input exceeds 64 MiB"));
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= deadline {
                    return Ok((bytes, true));
                }
                let mut poll = libc::pollfd {
                    fd,
                    events: libc::POLLIN,
                    revents: 0,
                };
                let ms = deadline
                    .saturating_duration_since(std::time::Instant::now())
                    .as_millis()
                    .clamp(1, 1000) as i32;
                // SAFETY: poll points at one writable pollfd; timeout is bounded.
                unsafe {
                    libc::poll(&mut poll, 1, ms);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
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

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;
