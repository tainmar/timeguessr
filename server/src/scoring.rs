//! Round scoring: up to 500 points for the year and 500 for the location.

use std::f64::consts::PI;

pub const MAX_POINTS: i32 = 500;

/// Mean Earth radius (IUGG), used both for distances and for the antipodal maximum
/// so that no distance can exceed [`MAX_DISTANCE_KM`].
pub const EARTH_RADIUS_KM: f64 = 6371.0088;

/// Half the Earth's circumference: the distance to the antipode (~20 015 km).
pub const MAX_DISTANCE_KM: f64 = PI * EARTH_RADIUS_KM;

/// Distance at which the location curve has lost ~10% of its points.
/// Small values make the curve sharper near the target.
const LOCATION_SCALE_KM: f64 = 0.5;

/// Number of years off at which the year score is halved.
const YEAR_HALF_POINTS_AT: f64 = 5.0;

/// Great-circle distance in kilometers (haversine).
pub fn distance_km(lat1: f64, lng1: f64, lat2: f64, lng2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dp = (lat2 - lat1).to_radians();
    let dl = (lng2 - lng1).to_radians();
    let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    let c = 2.0 * a.clamp(0.0, 1.0).sqrt().asin();
    (EARTH_RADIUS_KM * c).min(MAX_DISTANCE_KM)
}

/// Logarithmic decay: 500 at the exact spot, exactly 0 at the antipode, and the
/// slope keeps increasing as the guess gets closer (scale invariant, so it rewards
/// "right street" vs "right neighbourhood" as much as "right country" vs "right continent").
///
/// `500 · (1 − ln(1 + d/c) / ln(1 + D/c))` with c = 0.5 km and D = antipodal distance.
pub fn location_score(distance_km: f64) -> i32 {
    let d = distance_km.clamp(0.0, MAX_DISTANCE_KM);
    let ratio = (1.0 + d / LOCATION_SCALE_KM).ln() / (1.0 + MAX_DISTANCE_KM / LOCATION_SCALE_KM).ln();
    ((MAX_POINTS as f64) * (1.0 - ratio)).round().clamp(0.0, MAX_POINTS as f64) as i32
}

/// Cauchy-shaped decay: forgiving for ±1 year, steep in the middle, long tail.
///
/// `500 / (1 + (Δ/5)²)`: 0→500, 1→481, 2→431, 3→368, 5→250, 10→100, 20→29.
pub fn year_score(guess: i32, actual: i32) -> i32 {
    let diff = (guess - actual).abs() as f64;
    ((MAX_POINTS as f64) / (1.0 + (diff / YEAR_HALF_POINTS_AT).powi(2))).round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn year_score_anchors() {
        let expected = [(0, 500), (1, 481), (2, 431), (3, 368), (5, 250), (10, 100), (20, 29)];
        for (diff, points) in expected {
            assert_eq!(year_score(2000 + diff, 2000), points, "diff {diff}");
            assert_eq!(year_score(2000 - diff, 2000), points, "diff -{diff}");
        }
    }

    #[test]
    fn year_score_is_decreasing() {
        let mut prev = year_score(2000, 2000);
        for diff in 1..120 {
            let s = year_score(2000 + diff, 2000);
            // Strictly decreasing where it matters; integer rounding may plateau in the tail.
            assert!(s < prev || (diff > 20 && s == prev), "{diff}: {s} !< {prev}");
            prev = s;
        }
    }

    #[test]
    fn location_score_anchors() {
        assert_eq!(location_score(0.0), 500);
        assert_eq!(location_score(1.0), 448);
        assert_eq!(location_score(10.0), 356);
        assert_eq!(location_score(100.0), 250);
        assert_eq!(location_score(MAX_DISTANCE_KM), 0);
        assert_eq!(location_score(MAX_DISTANCE_KM * 2.0), 0);
    }

    #[test]
    fn location_score_is_convex_and_decreasing() {
        // Points lost per extra km shrink as the distance grows.
        let mut prev_loss_per_km = f64::INFINITY;
        for d in [0.0, 1.0, 10.0, 100.0, 1000.0, 10000.0] {
            let step = 1.0 + d;
            let loss = (location_score(d) - location_score(d + step)) as f64 / step;
            assert!(loss <= prev_loss_per_km, "{d}");
            prev_loss_per_km = loss;
        }
    }

    #[test]
    fn antipode_scores_zero() {
        let d = distance_km(48.8566, 2.3522, -48.8566, 2.3522 - 180.0);
        assert!((d - MAX_DISTANCE_KM).abs() < 1e-6, "{d}");
        assert_eq!(location_score(d), 0);
    }

    #[test]
    fn known_distance() {
        // Paris -> London ≈ 343.5 km
        let d = distance_km(48.8566, 2.3522, 51.5074, -0.1278);
        assert!((d - 343.5).abs() < 1.0, "{d}");
        // Across the antimeridian
        let d = distance_km(0.0, 179.5, 0.0, -179.5);
        assert!((d - 111.2).abs() < 0.5, "{d}");
    }
}
