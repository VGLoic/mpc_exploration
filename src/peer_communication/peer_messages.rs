use uuid::Uuid;

#[derive(Clone)]
pub enum PeerMessage {
    NewProcess { peer_id: u8, process_id: Uuid },
}

impl PeerMessage {
    pub fn new_process(peer_id: u8, process_id: Uuid) -> Self {
        Self::NewProcess {
            peer_id,
            process_id,
        }
    }

    pub fn peer_id(&self) -> u8 {
        match self {
            PeerMessage::NewProcess { peer_id, .. } => *peer_id,
        }
    }
}
