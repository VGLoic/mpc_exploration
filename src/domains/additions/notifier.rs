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

    pub async fn run_interval_ping(&self) {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(1_000));
        loop {
            interval.tick().await;
            if let Err(e) = self.channel_sender.try_send(()) {
                match e {
                    tokio::sync::mpsc::error::TrySendError::Full(_) => {
                        // It's fine, the channel is full, we can skip this ping
                    }
                    tokio::sync::mpsc::error::TrySendError::Closed(_) => {
                        tracing::warn!("Channel closed, stopping interval ping");
                        break;
                    }
                }
            }
        }
    }
}

impl Notifier for IntervalPing {
    fn ping(&self) {
        let _ = self.channel_sender.try_send(());
    }
}
