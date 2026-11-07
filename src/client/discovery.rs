use super::*;
fn candidates(paths: &Paths) -> Vec<(PathBuf, &'static str)> {
    let local = paths.runtime.join("sock");
    let source = if std::env::var_os("TMUX_SIDECAR_RUNTIME_DIR").is_some_and(|v| !v.is_empty()) {
        "runtime (TMUX_SIDECAR_RUNTIME_DIR)"
    } else {
        "default runtime"
    };
    let mut result = vec![(local.clone(), source)];
    if let Ok(bytes) = std::fs::read(paths.state.join("daemon.socket")) {
        use std::os::unix::ffi::OsStringExt;
        let recorded = PathBuf::from(std::ffi::OsString::from_vec(bytes));
        if recorded != local {
            result.push((recorded, "recorded state"));
        }
    }
    result
}
pub(crate) async fn connect_identified(
    paths: &Paths,
) -> io::Result<(UnixStream, PathBuf, &'static str)> {
    let mut errors = Vec::new();
    let mut kind = io::ErrorKind::NotFound;
    for (path, source) in candidates(paths) {
        let error = match tokio::time::timeout(
            Duration::from_millis(250),
            UnixStream::connect(&path),
        )
        .await
        {
            Ok(Ok(stream)) => return Ok((stream, path, source)),
            Ok(Err(error)) => error,
            Err(_) => io::Error::new(io::ErrorKind::TimedOut, "exceeded 250 ms"),
        };
        kind = error.kind();
        errors.push(format!("{source} socket {}: {error}", path.display()));
    }
    Err(io::Error::new(kind, errors.join("; ")))
}

pub(crate) async fn connect(paths: &Paths) -> io::Result<UnixStream> {
    connect_identified(paths).await.map(|(stream, _, _)| stream)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn recorded_socket_deduplicates_and_names_connection_errors() {
        let root = tempfile::tempdir().unwrap();
        let paths = Paths {
            runtime: root.path().join("runtime"),
            state: root.path().to_owned(),
            config: root.path().join("config"),
        };
        std::fs::write(
            paths.state.join("daemon.socket"),
            paths.runtime.join("sock").as_os_str().as_encoded_bytes(),
        )
        .unwrap();
        assert_eq!(candidates(&paths).len(), 1);
        let recorded = root.path().join("recorded-sock");
        std::fs::write(
            paths.state.join("daemon.socket"),
            recorded.as_os_str().as_encoded_bytes(),
        )
        .unwrap();
        let error = connect(&paths).await.unwrap_err().to_string();
        assert!(
            error.contains(&format!("recorded state socket {}", recorded.display())),
            "{error}"
        );
        assert!(
            error.contains("runtime")
                && error.contains(paths.runtime.join("sock").to_str().unwrap())
        );
    }
}
