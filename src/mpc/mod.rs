use std::collections::HashMap;

mod polynomial;

#[derive(Clone, Debug)]
pub struct Share {
    pub point: u8,
    pub value: u64,
}
pub fn split_secret(secret: u64, points: &[u8], n: u64) -> HashMap<u8, u64> {
    let mut coefficients = vec![secret];
    for _ in 1..points.len() {
        let coeff = rand::random::<u64>() % n;
        coefficients.push(coeff);
    }
    let poly = polynomial::Polynomial::new(coefficients);
    let mut shares = HashMap::new();
    for point in points {
        shares.insert(*point, poly.evaluate(*point as u64, n));
    }
    shares
}

pub fn recover_secret(shares: &[Share], n: u64) -> Result<u64, anyhow::Error> {
    let mut points = Vec::with_capacity(shares.len());
    let mut values = Vec::with_capacity(shares.len());
    for share in shares {
        points.push(share.point as u64);
        values.push(share.value);
    }

    let poly = polynomial::Polynomial::interpolate(&points, &values, n)?;

    Ok(poly.evaluate_at_zero())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_sharing() {
        let n = 1_000_000_007;
        let secret = rand::random::<u64>() % n;
        let points_len = rand::random::<u8>() % 100 + 3; // at least 3 points
        let points = (1..=points_len).collect::<Vec<u8>>();
        let shares = split_secret(secret, &points, n);
        let share_vec: Vec<Share> = shares
            .iter()
            .map(|(k, v)| Share {
                point: *k,
                value: *v,
            })
            .collect();
        let recovered_secret = recover_secret(&share_vec, n).unwrap();
        assert_eq!(secret, recovered_secret);
    }
}
