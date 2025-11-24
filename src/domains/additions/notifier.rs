/// A notifier trait and its implementation for sending pings through a channel.
/// This is used to notify other parts of the system at regular intervals or on demand.
pub trait Notifier: Send + Sync {
    /// Sends a ping notification.
    /// This method attempts to send a ping through the associated channel.
    fn ping(&self);
}

pub struct IntervalPing {
    channel_sender: tokio::sync::mpsc::Sender<()>,
}
impl IntervalPing {
    pub fn new(channel_sender: tokio::sync::mpsc::Sender<()>) -> Self {
        Self { channel_sender }
    }

    /// Runs the interval ping loop, sending pings at the specified interval.
    /// This method should be run in an asynchronous context.
    /// The loop will continue indefinitely until the channel is closed.
    /// # Arguments
    /// * `interval` - The duration between each ping.
    pub async fn run_interval_ping(&self, interval: std::time::Duration) {
        let mut interval = tokio::time::interval(interval);
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
        if let Err(e) = self.channel_sender.try_send(()) {
            match e {
                tokio::sync::mpsc::error::TrySendError::Full(_) => {
                    // It's fine, the channel is full, we can skip this ping
                }
                tokio::sync::mpsc::error::TrySendError::Closed(_) => {
                    tracing::warn!("Channel closed, cannot send ping");
                }
            }
        }
    }
}
