mod common;

use common::*;
use section_kernel::model::WorldScene;
use section_kernel::section::{build_section, SectionRequest};

fn req(normal: [f64; 3], offset: f64) -> SectionRequest {
    SectionRequest {
        normal,
        offset: Some(offset),
        origin: None,
        include_segments: Some(true),
    }
}

fn cut(objects: Vec<section_kernel::model::ObjectDef>, normal: [f64; 3], offset: f64) -> section_kernel::section::SectionResult {
    let s = scene(objects);
    let world = WorldScene::build(&s).expect("scene should bake");
    build_section(&req(normal, offset), &world).expect("section should build")
}

/// A plane that passes exactly through the two vertical mesh edges of a cube
/// (normal (1,-1,0), d=0). Before paired tracing this degree-4 junction split
/// the closed ring into two open chains; it must now stay one closed loop.
#[test]
fn plane_exactly_through_two_vertical_edges_stays_one_closed_loop() {
    let r = cut(
        vec![object("cube", "room-1", box_mesh([-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]))],
        [1.0, -1.0, 0.0],
        0.0,
    );
    assert_eq!(r.stats.open_chains, 0, "no open chain at the edge junction");
    let outers: Vec<_> = r.contours.iter().filter(|c| c.closed && c.kind == "outer").collect();
    assert_eq!(outers.len(), 1);
    assert_eq!(outers[0].points2d.len(), 4);
    assert_eq!(r.caps.len(), 1);
    // Every junction node is consumed exactly twice (one in, one out).
    for seg in outers[0].segments.as_ref().unwrap() {
        assert!(!seg.sources.is_empty());
    }
}

/// A tilted plane that crosses vertical edges at interior points still
/// produces a single closed polygon (six to eight intersection vertices).
#[test]
fn tilted_plane_through_edges_closes_with_paired_tracing() {
    let r = cut(
        vec![object("cube", "room-1", box_mesh([-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]))],
        [1.0, -1.0, 0.25],
        0.0,
    );
    assert_eq!(r.stats.open_chains, 0);
    assert_eq!(r.stats.closed_contours, 1);
    assert_eq!(r.caps.len(), 1);
    let outer = &r.contours[0];
    assert!(outer.points3d.len() >= 6, "tilted cut polygon has multiple corners");
    for p in &outer.points3d {
        let d = p[0] - p[1] + 0.25 * p[2];
        assert!(d.abs() <= r.tolerance.linear * 4.0 + 1e-12, "point on tilted plane: {d}");
    }
}

/// Four triangles fan-triangulating one square, all coplanar with the cut
/// plane, must emit exactly one four-corner outline: the three interior edges
/// cancel and no duplicated contour lines survive.
#[test]
fn fan_triangulated_coplanar_patch_emits_one_outline() {
    let positions = tri_positions(&[
        [4.0, 0.0, 3.0],
        [8.0, 0.0, 3.0],
        [8.0, 3.0, 3.0],
        [4.0, 3.0, 3.0],
        [6.0, 1.5, 3.0],
    ]);
    let mesh = section_kernel::model::MeshDef {
        positions,
        indices: vec![0, 1, 4, 1, 2, 4, 2, 3, 4, 3, 0, 4],
        face_ids: Some(vec![10, 10, 10, 10]),
    };
    let r = cut(vec![object("canopy", "edge-case", mesh)], [0.0, 0.0, 1.0], 3.0);
    assert_eq!(r.stats.coplanar_triangles, 4);
    let outers: Vec<_> = r.contours.iter().filter(|c| c.closed && c.kind == "outer").collect();
    assert_eq!(outers.len(), 1, "one patch outline");
    assert_eq!(outers[0].points2d.len(), 4, "interior fan edges do not appear");
    assert_eq!(r.stats.open_chains, 0);
    assert_eq!(r.caps.len(), 1);
    assert!(r.diagnostics.iter().any(|d| d.code == "coplanarFaces"));
}

/// The checked-in edge-case fixture must bake and yield closed sections for
/// both the through-vertex and coplanar cuts used by the release batch.
#[test]
fn edge_case_fixture_passes_both_acceptance_cuts() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures/edge-cases.json");
    let text = std::fs::read_to_string(path).expect("edge fixture exists");
    let scene: section_kernel::Scene = serde_json::from_str(&text).expect("fixture parses");
    let world = WorldScene::build(&scene).expect("fixture bakes");

    let through = build_section(&req([1.0, -1.0, 0.25], 0.0), &world).unwrap();
    assert!(through.stats.closed_contours >= 1);
    assert_eq!(through.stats.open_chains, 0);

    let coplanar = build_section(&req([0.0, 0.0, 1.0], 3.0), &world).unwrap();
    assert!(coplanar.stats.coplanar_triangles >= 4);
    assert!(coplanar.contours.iter().any(|c| c.object_ids.contains(&"coplanar-canopy".to_string()) && c.closed));
}
