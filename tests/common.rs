use tokio::process::Child;

pub struct ServerGuard(pub Child);
impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.0.start_kill();
    }
}
