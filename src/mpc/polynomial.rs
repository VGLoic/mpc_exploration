use anyhow::anyhow;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Polynomial {
    /// Coefficients in ascending order, i.e. [1, 2, 3] -> 1 + 2x + 3x^2
    coefficients: Vec<u64>,
}

impl Polynomial {
    pub fn new(coefficients: Vec<u64>) -> Self {
        let mut coefficients = coefficients;
        while let Some(c) = coefficients.last()
            && c == &0
        {
            coefficients.pop();
        }
        Self { coefficients }
    }

    pub fn evaluate(&self, point: u64, modulo: u64) -> u64 {
        let modulo_as_u128: u128 = modulo.into();
        let point_as_u128: u128 = point.into();
        let mut power_of_x = 1_u128;
        let mut result = 0_u128;
        for c in &self.coefficients {
            result = (result + power_of_x.wrapping_mul((*c).into())) % modulo_as_u128;
            power_of_x = power_of_x * point_as_u128 % modulo_as_u128;
        }
        result as u64
    }

    pub fn evaluate_at_zero(&self) -> u64 {
        if self.coefficients.is_empty() {
            return 0;
        }
        self.coefficients[0]
    }

    pub fn interpolate(points: &[u64], values: &[u64], modulo: u64) -> Result<Self, anyhow::Error> {
        if points.len() != values.len() {
            return Err(anyhow!("points and values must have the same length"));
        }
        let master_numerator = Self::interpolate_from_roots(points, modulo);

        let mut coefficients = vec![0; points.len()];

        let modulo_as_u128: u128 = modulo.into();

        for (&point, &value) in points.iter().zip(values) {
            let (numerator, _) = master_numerator.div(
                &Self {
                    coefficients: vec![modulo - point, 1],
                },
                modulo,
            )?;
            let weight = ((value as u128)
                * (modulo_inv(numerator.evaluate(point, modulo), modulo)? as u128)
                % modulo_as_u128) as u64;
            for (i, c) in numerator.coefficients.into_iter().enumerate() {
                coefficients[i] = ((coefficients[i] as u128 + c as u128 * weight as u128)
                    % modulo_as_u128) as u64;
            }
        }

        Ok(Self::new(coefficients))
    }

    fn div(&self, other: &Self, n: u64) -> Result<(Self, Self), anyhow::Error> {
        if other.coefficients.is_empty() {
            return Err(anyhow!("unable to divide by zero coefficients"));
        }
        if other.coefficients.len() > self.coefficients.len() {
            return Ok((Self::new(vec![]), self.clone()));
        }

        let self_degree = self.coefficients.len() - 1;
        let other_degree = other.coefficients.len() - 1;
        let quotient_degree = self_degree - other_degree;

        let inv_leading_other_coefficient = modulo_inv(other.coefficients[other_degree], n)?;

        let modulo_as_u128: u128 = n.into();

        let mut remainder_coefficients = self.coefficients.clone();
        let mut quotient_coefficients = vec![0; 1 + quotient_degree];

        // We eliminate the leading coefficient of `remainder` until `remainder` has a degree lower than `other`, i.e. it makes `self_degree - other_degree + 1` iterations
        // We iterate from 0 to =quotient_degree
        for i in 0..=quotient_degree {
            let leading_remainder_coefficient = remainder_coefficients[self_degree - i];

            let quotient_coefficient = (leading_remainder_coefficient as u128
                * inv_leading_other_coefficient as u128
                % modulo_as_u128) as u64;

            remainder_coefficients.pop();

            if quotient_coefficient != 0 {
                quotient_coefficients[quotient_degree - i] = quotient_coefficient;
                // Subtract `quotient_coefficient * other * x^(quotient_degree - i)` from `remainder`
                // Last one is skipped as we already popped it
                for (j, &c) in other.coefficients.iter().enumerate().take(other_degree) {
                    remainder_coefficients[quotient_degree - i + j] = modulo(
                        remainder_coefficients[quotient_degree - i + j] as i128
                            - c as i128 * quotient_coefficient as i128,
                        n,
                    );
                }
            }
        }

        Ok((
            Self::new(quotient_coefficients),
            Self::new(remainder_coefficients),
        ))
    }

    fn interpolate_from_roots(roots: &[u64], n: u64) -> Self {
        if roots.is_empty() {
            return Self {
                coefficients: vec![],
            };
        }

        let modulo_as_u128: u128 = n.into();

        let mut coefficients = Vec::with_capacity(roots.len());
        coefficients.push(1);
        for (i, &root) in roots.iter().enumerate() {
            // Leading coefficient is pushed one level higher
            coefficients.push(1);

            // Each existing coefficient is multiplied by x, hence coeff[j] += coeff[j - 1]
            // Additionally, each coefficient is multiplified by -root so coeff[j] *= neg_root
            // Combining both we have coeff[j] = coeff[j - 1] - root * coeff[j];
            // We iterate starting in reverse order and we take care of the 0 case at the end
            let neg_root = n - root;
            for j in (1..=i).rev() {
                coefficients[j] = modulo(
                    coefficients[j - 1] as i128 - coefficients[j] as i128 * root as i128,
                    n,
                )
            }
            coefficients[0] =
                ((coefficients[0] as u128 * neg_root as u128) % modulo_as_u128) as u64;
        }
        Polynomial { coefficients }
    }
}

/// Computes a^(-1) (mod n) using the Extended Euclidean Algorithm
/// Returns None if a has no inverse mod n (i.e. if gcd(a, n) != 1)
pub fn modulo_inv(a: u64, n: u64) -> Result<u64, anyhow::Error> {
    if a == 0 {
        return Err(anyhow!("0 does not have an index"));
    }
    if a == 1 {
        return Ok(1);
    }

    let (mut new_r, mut r) = ((a as i128), (n as i128));
    let (mut new_t, mut t) = (1_i128, 0_i128);

    while new_r != 0 {
        let q = r / new_r;
        (new_r, r) = (r - q * new_r, new_r);
        (new_t, t) = (t - q * new_t, new_t);
    }

    if r != 1 {
        return Err(anyhow!(
            "gcd of input: {a} with n: {n} is not one, unable to compute the inverse"
        ));
    }

    Ok(modulo(t, n))
}

fn modulo(a: i128, n: u64) -> u64 {
    let n_as_i128: i128 = n.into();
    if a > 0 {
        if a < n_as_i128 {
            return a as u64;
        }
        return (a % n_as_i128) as u64;
    }

    if a == 0 {
        return 0;
    }

    (n_as_i128 + a % n_as_i128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_polynomial_evaluation() {
        let poly = Polynomial::new(vec![3, 2, 1]); // 3 + 2x + 1x^2
        assert_eq!(poly.evaluate(0, 100), 3);
        assert_eq!(poly.evaluate(1, 100), 6);
        assert_eq!(poly.evaluate(2, 100), 11);
        assert_eq!(poly.evaluate(3, 100), 18);
    }

    #[test]
    fn test_polynomial_evaluation_with_modulo() {
        let poly = Polynomial::new(vec![100, 200, 300]); // 100 + 200x + 300x^2
        assert_eq!(poly.evaluate(1, 256), (100 + 200 + 300) % 256);
        assert_eq!(poly.evaluate(2, 256), (100 + 400 + 1200) % 256);
    }

    #[test]
    fn test_interpolate_from_roots() {
        let roots = (1..2000).collect::<Vec<u64>>();
        let poly = Polynomial::interpolate_from_roots(&roots, 1_000_000_007);
        for root in roots {
            assert_eq!(poly.evaluate(root, 1_000_000_007), 0);
        }
    }

    #[test]
    fn test_modulo_inv() {
        assert_eq!(modulo_inv(3, 11).unwrap(), 4); // 3 * 4 % 11 == 1
        assert_eq!(modulo_inv(10, 17).unwrap(), 12); // 10 * 12 % 17 == 1
        assert!(modulo_inv(2, 4).is_err()); // gcd(2, 4) != 1
    }

    #[test]
    fn test_modulo_inv_basic() {
        // 3 * 7 = 21 ≡ 1 mod 10, so inv(3, 10) = 7
        assert_eq!(modulo_inv(3, 10).unwrap(), 7);
        // 2 has no inverse mod 4
        assert!(modulo_inv(2, 4).is_err());
        // 1 is always its own inverse
        assert_eq!(modulo_inv(1, 13).unwrap(), 1);
        // 0 has no inverse
        assert!(modulo_inv(0, 13).is_err());
    }

    #[test]
    fn test_modulo_inv_large_prime() {
        let n = 1_000_000_007;
        let a = rand::random();
        let inv = modulo_inv(a, n).unwrap();
        assert_eq!(a as u128 * inv as u128 % n as u128, 1);
    }

    #[test]
    fn test_division() {
        let n: u64 = 1_000_000_007;
        let p1 = Polynomial::new(vec![0, 0, 0, 1, 0, 0, 1]); // x^6 + x^3
        let p2 = Polynomial::new(vec![1, 0, 0, 1]); // x^3 + 1
        let (quotient, remainder) = p1.div(&p2, n).unwrap();
        assert_eq!(quotient, Polynomial::new(vec![0, 0, 0, 1])); // x^3
        assert_eq!(remainder, Polynomial::new(vec![])); // 0

        let p1 = Polynomial::new(vec![1, 2, 0, 0, 0, 0, 1]); // x^6 + 2x + 1
        let p2 = Polynomial::new(vec![1, 0, 0, 1]); // x^3 + 1
        // x^6 + 2x + 1 = x^3 * (x^3 + 1) -x^3 + 2x + 1 = (x^3 - 1) * (x^3 + 1) + 2x + 2
        let (quotient, remainder) = p1.div(&p2, n).unwrap();
        assert_eq!(quotient, Polynomial::new(vec![n - 1, 0, 0, 1])); // x^3 - 1
        assert_eq!(remainder, Polynomial::new(vec![2, 2])); // 2x + 2
    }

    #[test]
    fn test_interpolation_from_coordinates() {
        let n: u64 = 1_000_000_007;
        let number_of_points: u64 = rand::random_range(2..=100);
        let points: Vec<u64> = (0..number_of_points).collect();
        let values: Vec<u64> = (0..number_of_points)
            .map(|_| {
                let y: u64 = rand::random();
                y % n
            })
            .collect();
        let p = Polynomial::interpolate(&points, &values, n).unwrap();
        for (x, y) in points.into_iter().zip(values) {
            assert_eq!(p.evaluate(x, n), y);
        }
    }
}
