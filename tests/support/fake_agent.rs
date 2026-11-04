//! Copied executable retains an actual agent comm while firing fixture hooks.
use std::{fs, path::PathBuf, process::{Command, Stdio}, time::Duration};
struct ChildGuard(std::process::Child);
impl Drop for ChildGuard { fn drop(&mut self) { let _=self.0.kill(); let _=self.0.wait(); } }
fn main() {
    let root=PathBuf::from(std::env::var_os("TMUX_SIDECAR_STATE_DIR").unwrap());
    let pid=std::process::id();
    loop {
        let request=root.join(format!("fire-{pid}"));
        if let Ok(text)=fs::read_to_string(&request) {
            fs::remove_file(&request).unwrap();
            let mut lines=text.lines();
            let binary=lines.next().unwrap(); let format=lines.next().unwrap(); let fixture=lines.next().unwrap();
            let start=std::time::Instant::now();
            let mut command=Command::new(binary);
            command.args(["hook",format]).stdout(Stdio::piped()).stderr(Stdio::piped());
            if format=="codex-notify" {command.arg(fs::read_to_string(fixture).unwrap());}
            else {command.stdin(fs::File::open(fixture).unwrap());}
            let mut child=ChildGuard(command.spawn().unwrap());
            let status=loop {
                if let Some(status)=child.0.try_wait().unwrap() {break status;}
                assert!(start.elapsed()<Duration::from_secs(2)); std::thread::sleep(Duration::from_millis(1));
            };
            let output=child.0.stdout.take().unwrap(); let error=child.0.stderr.take().unwrap();
            use std::io::Read;
            let mut stdout=String::new(); let mut stderr=String::new();
            std::io::BufReader::new(output).read_to_string(&mut stdout).unwrap();
            std::io::BufReader::new(error).read_to_string(&mut stderr).unwrap();
            fs::write(root.join(format!("fired-{pid}")),format!("{}\n{}\n{}\n{}",status.success(),start.elapsed().as_millis(),stdout.len(),stderr.len())).unwrap();
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
