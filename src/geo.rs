//! Web Mercator, the projection of the tiles: the world is the unit square,
//! x east from 180° W, y south from 85.05° N.

use std::f64::consts::PI;

pub const MAX_LATITUDE: f64 = 85.051_128_78;
/// The equator in meters, on the sphere Web Mercator uses.
pub const EQUATOR_M: f64 = 40_075_016.686;
/// A nautical mile in meters.
pub const NM: f64 = 1852.0;

pub fn mercator(lon: f64, lat: f64) -> [f64; 2] {
    let phi = lat.clamp(-MAX_LATITUDE, MAX_LATITUDE).to_radians();
    [(lon + 180.0) / 360.0, (1.0 - phi.tan().asinh() / PI) / 2.0]
}

pub fn lon_lat(p: [f64; 2]) -> (f64, f64) {
    let lon = p[0] * 360.0 - 180.0;
    let lat = (PI * (1.0 - 2.0 * p[1])).sinh().atan().to_degrees();
    (lon, lat)
}

/// Ground meters per logical pixel at a zoom level, at a latitude.
pub fn meters_per_pixel(zoom: f64, lat: f64) -> f64 {
    EQUATOR_M * lat.to_radians().cos() / (256.0 * zoom.exp2())
}

/// The chart scale a zoom level shows at a latitude, as the denominator:
/// 12000 for 1:12,000. A logical pixel is taken as 0.28 mm, as S-52 does.
pub fn scale_denominator(zoom: f64, lat: f64) -> f64 {
    meters_per_pixel(zoom, lat) / 0.000_28
}

/// The distance between two positions in nautical miles, over the ground:
/// the great circle on a sphere of the earth's mean radius. Mercator
/// distances are no use for this — the projection stretches with latitude.
pub fn distance_nm(a: (f64, f64), b: (f64, f64)) -> f64 {
    const RADIUS_NM: f64 = 6_371_008.8 / NM;
    let (lat1, lat2) = (a.0.to_radians(), b.0.to_radians());
    let half_lat = ((lat2 - lat1) / 2.0).sin();
    let half_lon = ((b.1 - a.1).to_radians() / 2.0).sin();
    let h = half_lat * half_lat + lat1.cos() * lat2.cos() * half_lon * half_lon;
    2.0 * RADIUS_NM * h.clamp(0.0, 1.0).sqrt().asin()
}

/// A rectangle in Web Mercator units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Rect {
    pub const EMPTY: Rect = Rect {
        x0: f64::INFINITY,
        y0: f64::INFINITY,
        x1: f64::NEG_INFINITY,
        y1: f64::NEG_INFINITY,
    };

    pub fn tile(z: u32, x: u32, y: u32) -> Rect {
        let n = f64::from(z).exp2();
        Rect {
            x0: f64::from(x) / n,
            y0: f64::from(y) / n,
            x1: f64::from(x + 1) / n,
            y1: f64::from(y + 1) / n,
        }
    }

    pub fn add(&mut self, p: [f64; 2]) {
        self.x0 = self.x0.min(p[0]);
        self.y0 = self.y0.min(p[1]);
        self.x1 = self.x1.max(p[0]);
        self.y1 = self.y1.max(p[1]);
    }

    pub fn union(&mut self, other: &Rect) {
        self.x0 = self.x0.min(other.x0);
        self.y0 = self.y0.min(other.y0);
        self.x1 = self.x1.max(other.x1);
        self.y1 = self.y1.max(other.y1);
    }

    pub fn is_empty(&self) -> bool {
        self.x0 > self.x1 || self.y0 > self.y1
    }

    pub fn intersects(&self, o: &Rect) -> bool {
        self.x0 <= o.x1 && o.x0 <= self.x1 && self.y0 <= o.y1 && o.y0 <= self.y1
    }

    pub fn contains(&self, p: [f64; 2]) -> bool {
        p[0] >= self.x0 && p[0] <= self.x1 && p[1] >= self.y0 && p[1] <= self.y1
    }

    pub fn expand(&self, by: f64) -> Rect {
        Rect {
            x0: self.x0 - by,
            y0: self.y0 - by,
            x1: self.x1 + by,
            y1: self.y1 + by,
        }
    }

    pub fn center(&self) -> [f64; 2] {
        [(self.x0 + self.x1) / 2.0, (self.y0 + self.y1) / 2.0]
    }
}

/// Whether a point lies inside rings filled even-odd.
pub fn inside(rings: &[Vec<[f64; 2]>], p: [f64; 2]) -> bool {
    let mut odd = false;
    for ring in rings {
        let n = ring.len();
        if n < 3 {
            continue;
        }
        let mut j = n - 1;
        for i in 0..n {
            let (a, b) = (ring[i], ring[j]);
            if (a[1] > p[1]) != (b[1] > p[1])
                && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
            {
                odd = !odd;
            }
            j = i;
        }
    }
    odd
}

/// Distance in Mercator units from a point to a segment.
pub fn segment_distance(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (qx, qy) = (a[0] + t * dx, a[1] + t * dy);
    ((p[0] - qx).powi(2) + (p[1] - qy).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mercator_round_trips() {
        let p = mercator(-122.3148, 37.8663);
        let (lon, lat) = lon_lat(p);
        assert!((lon + 122.3148).abs() < 1e-9 && (lat - 37.8663).abs() < 1e-9);
        assert_eq!(mercator(0.0, 0.0), [0.5, 0.5]);
    }

    #[test]
    fn scale_at_the_bay() {
        // Zoom 15 at the Bay is about 1:14,000, a harbor chart.
        let s = scale_denominator(15.0, 37.87);
        assert!((13_000.0..15_000.0).contains(&s), "{s}");
    }

    #[test]
    fn a_minute_of_latitude_is_a_mile() {
        let nm = distance_nm((37.0, -122.0), (37.0 + 1.0 / 60.0, -122.0));
        assert!((nm - 1.0).abs() < 0.002, "{nm}");
        // A minute of longitude is shorter by the cosine of the latitude.
        let east = distance_nm((37.0, -122.0), (37.0, -122.0 + 1.0 / 60.0));
        assert!((east - 37.0f64.to_radians().cos()).abs() < 0.002, "{east}");
        assert_eq!(distance_nm((37.0, -122.0), (37.0, -122.0)), 0.0);
    }

    #[test]
    fn even_odd_holes() {
        let outer = vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let hole = vec![[4.0, 4.0], [6.0, 4.0], [6.0, 6.0], [4.0, 6.0]];
        let rings = vec![outer, hole];
        assert!(inside(&rings, [2.0, 2.0]));
        assert!(!inside(&rings, [5.0, 5.0]));
        assert!(!inside(&rings, [12.0, 5.0]));
    }
}
