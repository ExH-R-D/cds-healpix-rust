//! Module containing spherical geometry structures and methods like 3D vectors, 
//! polygon or cone on the unit sphere...

pub mod coo3d; // made public for polygon query et webasembly
pub(super) mod cone;
pub(super) mod zone;
pub(super) mod elliptical_cone;
pub(super) mod proj;
pub(super) mod frame;

use std::f64::consts::{PI};
use super::TWICE_PI;

use self::coo3d::{Vect3, Vec3, UnitVect3, LonLat, LonLatT, Coo3D, cross_product, dot_product};

trait ContainsSouthPoleComputer {
  fn contains_south_pole(&self, polygon: &Polygon) -> bool;
}

/// We explicitly tell that the south pole is inside the polygon.
struct ProvidedTrue;
impl ContainsSouthPoleComputer for ProvidedTrue {
  #[inline]
  fn contains_south_pole(&self, _polygon: &Polygon ) -> bool {
    true
  }
}

/// We explicitly tell that the south pole is NOT inside the polygon.
struct ProvidedFalse;
impl ContainsSouthPoleComputer for ProvidedFalse {
  #[inline]
  fn contains_south_pole(&self, _polygon: &Polygon) -> bool {
    false
  }
}
/// Consider the south pole to be inside the polygon only if both poles are in different polygon
/// area (i.e. one in the polygon, one in its complementary) and the gravity center of the polygon
/// vertices is in the south hemisphere.
struct Basic;
impl ContainsSouthPoleComputer for Basic {
  fn contains_south_pole(&self, polygon: &Polygon) -> bool {
    let mut gravity_center_z = 0.0_f64;
    let mut sum_delta_lon = 0.0_f64;
    let mut j = (polygon.vertices.len() - 1) as usize;
    let to = j; // variable defined to remove a clippy warning
    for i in 0..=to {
      let delta_lon = polygon.vertices[i].lon() - polygon.vertices[j].lon();
      let abs_delta_lon = delta_lon.abs();
      if abs_delta_lon <= PI {
        sum_delta_lon += delta_lon;
      } else if delta_lon > 0.0 {
        sum_delta_lon -= TWICE_PI - abs_delta_lon;
      } else {
        sum_delta_lon += TWICE_PI - abs_delta_lon;
      }
      gravity_center_z += polygon.vertices[i].z();
      j = i;
    }
    sum_delta_lon.abs() > PI // Both poles in both the polygon and its complementary
      && gravity_center_z < 0.0 // Polygon vertices gravity center in south hemisphere
  }
}

static PROVIDED_TRUE: ProvidedTrue = ProvidedTrue;
static PROVIDED_FALSE: ProvidedFalse = ProvidedFalse;
static BASIC: Basic = Basic;
// Gravity center of the 3 first vertices?

pub enum ContainsSouthPoleMethod {
  Default,
  DefaultComplement,
  ContainsSouthPole,
  DoNotContainsSouthPole,
  ControlPointIn(Coo3D),
  ControlPointOut(Coo3D),
  // Opposite of the gravity center out of the polygon?
}

impl ContainsSouthPoleComputer for ContainsSouthPoleMethod {
  fn contains_south_pole(&self, polygon: &Polygon) -> bool {
    match self {
      ContainsSouthPoleMethod::Default =>
        BASIC.contains_south_pole(polygon),
      ContainsSouthPoleMethod::DefaultComplement =>
        !BASIC.contains_south_pole(polygon),
      ContainsSouthPoleMethod::ContainsSouthPole =>
        PROVIDED_TRUE.contains_south_pole(polygon),
      ContainsSouthPoleMethod::DoNotContainsSouthPole =>
        PROVIDED_FALSE.contains_south_pole(polygon),
      ContainsSouthPoleMethod::ControlPointIn(control_point) =>
        if polygon.contains(control_point) {
          polygon.contains_south_pole
        } else {
          !polygon.contains_south_pole
        },
      ContainsSouthPoleMethod::ControlPointOut(control_point) =>
        if polygon.contains(control_point) {
          !polygon.contains_south_pole
        } else {
          polygon.contains_south_pole
        },
    }
  }
}

pub struct Polygon {
  vertices: Box<[Coo3D]>,
  cross_products: Box<[Vect3]>,
  contains_south_pole: bool,  
}

impl Polygon {
  
  pub fn new(vertices: Box<[LonLat]>) -> Polygon {
    Polygon::new_custom(vertices, &ContainsSouthPoleMethod::Default)
  }

  pub fn new_custom(vertices: Box<[LonLat]>, method: &ContainsSouthPoleMethod) -> Polygon {
    let vertices: Box<[Coo3D]> = lonlat2coo3d(&vertices);
    let cross_products: Box<[Vect3]> = compute_cross_products(&vertices);
    let mut polygon = Polygon {
      vertices,
      cross_products,
      contains_south_pole: false,
    };
    polygon.contains_south_pole = method.contains_south_pole(&polygon);
    polygon
  }

  /// Control point must be in the polygon.
  /// We typically use the gravity center.
  pub fn must_contain(&mut self, control_point: &UnitVect3) {
    if !self.contains(&Coo3D::from_ref(control_point)) {
      self.contains_south_pole = !self.contains_south_pole;
    }
  }

  pub fn vertices(&self) -> &[Coo3D] {
    &self.vertices
  } 
  
  /// Returns `true` if the polygon contain the point of given coordinates `coo`.
  #[inline]
  pub fn contains(&self, coo: &Coo3D) -> bool {
    self.contains_south_pole ^ self.odd_num_intersect_going_south(coo)
  }
  
  
  /// Returns `true` if an edge of the polygon intersects the great-circle arc defined by the 
  /// two given points (we consider the arc having a length < PI).
  pub fn intersect_great_circle_arc(&self, a: &Coo3D, b: &Coo3D) -> bool {
    // double ua, ub;
    // Ensure a < b in longitude
    let mut a = a;
    let mut b = b;
    if a.lon() > b.lon() {
      std::mem::swap(&mut a, &mut b);
    }
    // 
    let n_vertices = self.vertices.len();
    let mut left = &self.vertices[n_vertices - 1];
    for i in 0..n_vertices {
      let right = &self.vertices[i];
      {
        // Ensures pA < pB in longitude
        let mut pa = left;
        let mut pb = right;
        if pa.lon() > pb.lon() {
          std::mem::swap(&mut pa, &mut pb);
        }
        if great_circle_arcs_are_overlapping_in_lon(a, b, pa, pb) {
          let ua = dot_product(a, &self.cross_products[i]);
          let ub = dot_product(b, &self.cross_products[i]);
          if polygon_edge_intersects_great_circle(ua, ub)
              && intersect_point_in_polygon_great_circle_arc(a, b, pa, pb, ua, ub) {
            return true;
          }
        }
      }
      left = right;
    }
    false
  }
  
  
  #[inline]
  fn odd_num_intersect_going_south(&self, coo: &Coo3D) -> bool {
    let mut c = false;
    let n_vertices = self.vertices.len();
    let mut left = &self.vertices[n_vertices - 1];
    for i in 0..n_vertices {
      let right = &self.vertices[i];
      if is_in_lon_range(coo,  left,  right)
        && cross_plane_going_south(coo, &self.cross_products[i]) {
        c = !c;
      }
      left = right;
    }
    c
  }
  
}

#[inline]
fn lonlat2coo3d(vertices: &[LonLat]) -> Box<[Coo3D]> {
  vertices.iter()
    .map(|lonlat| Coo3D::from_sph_coo(lonlat.lon, lonlat.lat))
    .collect::<Vec<Coo3D>>()
    .into_boxed_slice()
}

#[inline]
fn compute_cross_products(vertices: &[Coo3D]) -> Box<[Vect3]> {
  let mut i = (vertices.len() - 1) as usize;
  let cross_products: Vec<Vect3> = (0..=i)
    .map(|j| {
      let v1 = vertices.get(i).unwrap();
      let v2 = vertices.get(j).unwrap();
      i = j;
      cross_product(v1, v2)
    })
    .map(|v|
      if v.z() < 0.0 {
        v.opposite()
      } else {
        v
      })
    .collect();
  cross_products.into_boxed_slice()
}



/// Returns `true` if the given point `p` longitude is between the given vertices `v1` and `v2`
/// longitude range.rust 
#[inline]
fn is_in_lon_range(coo: &Coo3D, v1: &Coo3D, v2: &Coo3D) -> bool {
  // First version of the code: 
  //   ((v2.lon() - v1.lon()).abs() > PI) != ((v2.lon() > coo.lon()) != (v1.lon() > coo.lon()))
  // 
  // Lets note 
  //   - lonA = v1.lon()
  //   - lonB = v2.lon()
  //   - lon0 = coo.lon()
  // When (lonB - lonA).abs() <= PI 
  //   => lonB > lon0 != lonA > lon0  like in PNPOLY
  //   A    B    lonA <= lon0 && lon0 < lonB
  // --[++++[--
  //   B    A    lonB <= lon0 && lon0 < lonA
  //
  // But when (lonB - lonA).abs() > PI, then the test should be 
  //  =>   lonA >= lon0 == lonB >= lon0 
  // <=> !(lonA >= lon0 != lonB >= lon0)
  //    A  |  B    (lon0 < lonB) || (lonA <= lon0)
  //  --[++|++[--
  //    B  |  A    (lon0 < lonA) || (lonB <= lon0)
  //
  // Instead of lonA > lon0 == lonB > lon0,
  //     i.e. !(lonA > lon0 != lonB > lon0).
  //    A  |  B    (lon0 <= lonB) || (lonA < lon0)
  //  --]++|++]--
  //    B  |  A    (lon0 <= lonA) || (lonB < lon0)
  //
  // So the previous code was bugged in this very specific case: 
  // - `lon0` has the same value as a vertex being part of:
  //   - one segment that do not cross RA=0
  //   - plus one segment crossing RA=0.
  //   - the point have an odd number of intersections with the polygon 
  //     (since it will be counted 0 or 2 times instead of 1).
  let dlon = v2.lon() - v1.lon();
  if dlon < 0.0 {
    (dlon >= -PI) == (v2.lon() <= coo.lon() && coo.lon() < v1.lon())
  } else {
    (dlon <=  PI) == (v1.lon() <= coo.lon() && coo.lon() < v2.lon())
  }
}

/// Returns `true` if the two given great_circle arcs have their longitudes ranges which overlaps.
/// # WARNING:
/// We **MUST** have:
/// * `pa.lon() <= pb.lon()`
/// * `a.lon() <= b.lon()`
fn great_circle_arcs_are_overlapping_in_lon(a: &Coo3D, b: &Coo3D, pa: &Coo3D, pb: &Coo3D) -> bool {
  debug_assert!( a.lon() <=  b.lon());
  debug_assert!(pa.lon() <= pb.lon());
  match (b.lon() - a.lon() < PI, pb.lon() - pa.lon() <= PI) {
    (true, true)   => pb.lon() >= a.lon() && pa.lon() <= b.lon(), //  a - b  AND  pa - pb
    (true, false)  => pb.lon() <= b.lon() || pa.lon() >= a.lon(), //  a - b  AND pb -|- pa
    (false, true)  => pb.lon() >= b.lon() || pa.lon() <= a.lon(), // b -|- a AND  pa - pb
    (false, false) => true, // b -|- a AND pb -|- pa
  }
  /* Test if their is a perf differenc (should be neglectable!!)
  if b.lon() - a.lon() < PI {
    if pb.lon() - pa.lon() <= PI {
      a.lon() <= pb.lon() && b.lon() >= pa.lon()
    } else {
      pb.lon() <= b.lon() || pa.lon() >= a.lon()
    }
  } else {
    (pb.lon() - pa.lon() > PI) || pb.lon() >= b.lon() || pa.lon() <= a.lon()
  }*/
}

/// Returns `true` if the line at constant `(x, y)` and decreasing `z` going from the given point
/// toward south intersect the plane of given normal vector. The normal vector must have a positive
/// z coordinate (=> must be in the north hemisphere)
#[inline]
fn cross_plane_going_south<T1, T2>(coo: &T1, plane_normal_dir_in_north_hemisphere: &T2) -> bool
  where T1: Vec3, T2: Vec3 {
  dot_product(coo, plane_normal_dir_in_north_hemisphere) > 0.0
}

/// Tells if the great-circle arc from vector a to vector b intersects the plane of the 
/// great circle defined by its normal vector N.
/// # Inputs:
/// - `a_dot_edge_normal` the dot product of vector a with the great circle normal vector N.
/// - `b_dot_edge_normal` the dot product of vector b with the great circle normal vector N.
/// # Output:
///  - `true` if vectors a and b are in opposite part of the plane having for normal vector vector N.
#[inline]
fn polygon_edge_intersects_great_circle(a_dot_edge_normal: f64, b_dot_edge_normal: f64) -> bool {
  (a_dot_edge_normal > 0.0) != (b_dot_edge_normal > 0.0)
}


/// Tells if the intersection line (i) between the two planes defined by vector a, b and pA, pB
/// respectively is inside the zone `[pA, pB]`.
fn intersect_point_in_polygon_great_circle_arc(
  a: &Coo3D, b: &Coo3D, pa: &Coo3D, pb: &Coo3D, a_dot_edge_normal: f64, b_dot_edge_normal: f64) -> bool {
  let intersect = normalized_intersect_point(a, b, a_dot_edge_normal, b_dot_edge_normal);
  let papb = dot_product(pa, pb);
  dot_product(pa,&intersect).abs() > papb && dot_product(pb, &intersect).abs() > papb
}


#[allow(clippy::many_single_char_names)]
fn normalized_intersect_point(a: &Coo3D, b: &Coo3D, a_dot_edge_normal: f64, b_dot_edge_normal: f64) -> UnitVect3 {
  // We note u = pA x pB
  // Intersection vector i defined by
  // lin
  // i = i / ||i|| 
  let x = b_dot_edge_normal * a.x() - a_dot_edge_normal * b.x();
  let y = b_dot_edge_normal * a.y() - a_dot_edge_normal * b.y();
  let z = b_dot_edge_normal * a.z() - a_dot_edge_normal * b.z();
  let norm = (x * x + y * y + z * z).sqrt();
  // Warning, do not consider the opposite vector!!
  UnitVect3::new_unsafe(x / norm, y / norm, z / norm)
}



#[cfg(test)]
mod tests {
  use super::*;
  use super::super::nested::{vertices};
  
  #[test]
  fn test_vec3() {
    let mut v = Vect3::new(1.0, 2.0, 3.0);
    assert_eq!("x: 1; y: 2; z: 3", format!("x: {}; y: {}; z: {}", Vec3::x(&v), Vec3::y(&v), Vec3::z(&v)));
    {
      let vref = &v;
      assert_eq!("x: 1; y: 2; z: 3", format!("x: {}; y: {}; z: {}", vref.x(), vref.y(), vref.z()));
    }
    let vrefmut = &mut v;
    assert_eq!("x: 1; y: 2; z: 3", format!("x: {}; y: {}; z: {}", vrefmut.x(), vrefmut.y(), vrefmut.z()));
  }
  
  #[test]
  fn testok_is_in_polygon() {
    let v = [(0.0, 0.0), (0.0, 0.5), (0.25, 0.25)];
    let poly = Polygon::new(
      v.iter().map(|(lon, lat)| LonLat { lon: *lon, lat: *lat} )
        .collect::<Vec<LonLat>>().into_boxed_slice()
    );
    let depth = 3_u8;
    let hash = 305_u64;
    let [(l_south, b_south), (l_east, b_east), (l_north, b_north), (l_west, b_west)] = vertices(depth, hash);
    let v = [
      Coo3D::from_sph_coo(l_south, b_south),
      Coo3D::from_sph_coo(l_east, b_east),
      Coo3D::from_sph_coo(l_north, b_north),
      Coo3D::from_sph_coo(l_west, b_west)
    ];
    assert_eq!(poly.contains(&v[0]), false);
    assert_eq!(poly.contains(&v[1]), false);
    assert_eq!(poly.contains(&v[2]), true);
    assert_eq!(poly.contains(&v[3]), true);
    
  }
  
}