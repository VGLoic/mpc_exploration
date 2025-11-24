pub trait Notifier: Send + Sync {
    fn ping(&self);
}

pub struct IntervalPing {
    channel_sender: tokio::sync::mpsc::Sender<()>,
}
impl IntervalPing {
    pub fn new(channel_sender: tokio::sync::mpsc::Sender<()>) -> Self {
        Self { channel_sender }
    }

    pub async fn run_interval_ping(&self) -> Result<(), anyhow::Error> {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(1_000));
        loop {
            interval.tick().await;
            if let Err(e) = self.channel_sender.try_send(()) {
                tracing::warn!("Error sending ping to the sender channel: {}", e);
            }
        }
    }
}

impl Notifier for IntervalPing {
    fn ping(&self) {
        let _ = self.channel_sender.try_send(());
    }
}
