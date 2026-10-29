//! Median und Perzentile an einer Stelle für messen, pfad, wlanverlauf und live. Issue #75.

/// Median; `None` for no values. Even count: mean of the two middle values.
pub fn median(v: &[f64]) -> Option<f64> {
    let s = sortiert(v)?;
    let n = s.len();
    Some(if n % 2 == 1 {
        s[n / 2]
    } else {
        (s[n / 2 - 1] + s[n / 2]) / 2.0
    })
}

/// Nearest-rank percentile (`p` in 1..=100); `None` for no values.
pub fn perzentil(v: &[f64], p: u32) -> Option<f64> {
    let s = sortiert(v)?;
    let rang = (s.len() * p.clamp(1, 100) as usize).div_ceil(100).max(1);
    Some(s[rang - 1])
}

fn sortiert(v: &[f64]) -> Option<Vec<f64>> {
    let mut s: Vec<f64> = v.iter().copied().filter(|x| x.is_finite()).collect();
    if s.is_empty() {
        return None;
    }
    s.sort_by(f64::total_cmp);
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_und_perzentil() {
        assert_eq!(median(&[]), None);
        assert_eq!(median(&[3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(&[4.0, 1.0, 3.0, 2.0]), Some(2.5));
        let v: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(perzentil(&v, 95), Some(95.0));
        assert_eq!(perzentil(&v, 5), Some(5.0));
        assert_eq!(perzentil(&[7.0], 95), Some(7.0));
        assert_eq!(perzentil(&[f64::NAN], 50), None);
    }
}
