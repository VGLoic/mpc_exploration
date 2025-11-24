#[derive(Clone)]
pub enum PeerMessage {
    NotifyProcessProgress { peer_id: u8 },
}

impl PeerMessage {
    pub fn notify_process_progress(peer_id: u8) -> Self {
        Self::NotifyProcessProgress { peer_id }
    }

    pub fn peer_id(&self) -> u8 {
        match self {
            PeerMessage::NotifyProcessProgress { peer_id } => *peer_id,
        }
    }
}
