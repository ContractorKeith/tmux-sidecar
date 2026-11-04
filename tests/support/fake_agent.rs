//! Test-only executable copied under each agent name; M2 adds hook fixtures.
fn main() {
    loop {
        std::thread::park_timeout(std::time::Duration::from_secs(60));
    }
}
