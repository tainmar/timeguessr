//! Round scoring: up to 500 points for the year and 500 for the location.

use std::f64::consts::PI;

pub const MAX_POINTS: i32 = 500;

/// Mean Earth radius (IUGG), used both for distances and for the antipodal maximum
/// so that no distance can exceed [`MAX_DISTANCE_KM`].
pub const EARTH_RADIUS_KM: f64 = 6371.0088;

/// Half the Earth's circumference: the distance to the antipode (~20 015 km).
pub const MAX_DISTANCE_KM: f64 = PI * EARTH_RADIUS_KM;

/// Decay length of the location curve: 1,500 km away keeps 1/e (~37%) of the points.
const LOCATION_SCALE_KM: f64 = 1500.0;

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

/// Exponential decay, steepest near the target: forgiving within a city, about half
/// the points in the right country 1,000 km away, and 0 from ~10,000 km (so at the antipode).
///
/// `500 · e^(−d / 1500 km)`: 3 km→499, 100 km→468, 500 km→358, 1,000 km→257, 2,000 km→132.
pub fn location_score(distance_km: f64) -> i32 {
    let d = distance_km.clamp(0.0, MAX_DISTANCE_KM);
    ((MAX_POINTS as f64) * (-d / LOCATION_SCALE_KM).exp()).round() as i32
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
        let expected = [
            (0, 500),
            (1, 481),
            (2, 431),
            (3, 368),
            (5, 250),
            (10, 100),
            (20, 29),
        ];
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
            assert!(
                s < prev || (diff > 20 && s == prev),
                "{diff}: {s} !< {prev}"
            );
            prev = s;
        }
    }

    #[test]
    fn location_score_anchors() {
        let expected = [
            (0.0, 500),
            (3.0, 499),
            (10.0, 497),
            (100.0, 468),
            (500.0, 358),
            (1000.0, 257),
            (2000.0, 132),
            (5000.0, 18),
            (10_500.0, 0),
            (MAX_DISTANCE_KM, 0),
            (MAX_DISTANCE_KM * 2.0, 0),
        ];
        for (d, points) in expected {
            assert_eq!(location_score(d), points, "{d} km");
        }
    }

    #[test]
    fn location_score_is_decreasing_and_steepest_close_by() {
        let mut prev = location_score(0.0);
        for d in (10..=12_000).step_by(10) {
            let s = location_score(d as f64);
            assert!(s <= prev, "{d}: {s} > {prev}");
            prev = s;
        }
        // The first 500 km cost more than the next 500 km, and so on.
        let losses: Vec<i32> = [0.0, 500.0, 1000.0, 1500.0, 2000.0]
            .windows(2)
            .map(|w| location_score(w[0]) - location_score(w[1]))
            .collect();
        assert!(losses.windows(2).all(|w| w[0] > w[1]), "{losses:?}");
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
