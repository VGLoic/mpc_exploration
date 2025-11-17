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
    println!("Recovering secret from shares: {:?}", shares);
    let mut points = Vec::with_capacity(shares.len());
    let mut values = Vec::with_capacity(shares.len());
    for share in shares {
        points.push(share.point as u64);
        values.push(share.value);
    }

    let poly = polynomial::Polynomial::interpolate(&points, &values, n)?;

    Ok(poly.evaluate_at_zero())
}
