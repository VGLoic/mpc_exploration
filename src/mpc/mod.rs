use std::collections::HashMap;

#[derive(Clone)]
pub struct Share {
    pub point: u8,
    pub value: u64,
}
pub fn split_secret(secret: u64, points: &[u8]) -> HashMap<u8, u64> {
    let mut shares = HashMap::new();
    for point in points {
        shares.insert(*point, secret);
    }
    shares
}

pub fn recover_secret(shares: &[Share]) -> u64 {
    shares.iter().map(|s| s.value).sum::<u64>() / (shares.len() as u64)
}
