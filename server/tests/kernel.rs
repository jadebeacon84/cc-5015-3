mod common;

use common::*;
use section_kernel::model::WorldScene;
use section_kernel::section::{build_section, SectionRequest, SectionResult};

fn req(normal: [f64; 3], offset: f64) -> SectionRequest {
    SectionRequest {
        normal,
        offset: Some(offset),
        origin: None,
        include_segments: Some(true),
    }
}

fn cut(objects: Vec<section_kernel::model::ObjectDef>, normal: [f64; 3], offset: f64) -> SectionResult {
    let scene = scene(objects);
    let world = WorldScene::build(&scene).expect("scene should bake");
    build_section(&req(normal, offset), &world).expect("section should build")
}

fn assert_close(a: f64, b: f64, what: &str) {
    assert!(
        (a - b).abs() <= 1e-6 * b.abs().max(1.0),
        "{what}: expected {b}, got {a}"
    );
}

fn closed_outer(r: &SectionResult) -> Vec<&section_kernel::section::ContourOut> {
    r.contours
        .iter()
        .filter(|c| c.closed && c.kind == "outer")
        .collect()
}

#[test]
fn solid_box_cut_midheight_yields_one_closed_rectangle() {
    let r = cut(
        vec![object("box", "room-1", box_mesh([0.0, 0.0, 0.0], [2.0, 2.0, 2.0]))],
        [0.0, 0.0, 1.0],
        0.5,
    );

    let outers = closed_outer(&r);
    assert_eq!(outers.len(), 1, "one outer contour");
    assert_eq!(outers[0].points2d.len(), 4, "rectangle has four corners");
    assert_close(outers[0].area_square_meters.unwrap(), 4.0, "rectangle area");
    assert_eq!(r.caps.len(), 1);
    assert_close(r.caps[0].area_square_meters, 4.0, "cap area");
    assert_eq!(r.stats.open_chains, 0);
    assert_eq!(r.stats.holes, 0);
    // The four corners must be exactly the box corners at z=0.5.
    let mut pts: Vec<[f64; 3]> = outers[0].points3d.clone();
    pts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    for p in &pts {
        assert_close(p[2], 0.5, "corner plane height");
        assert!(
            (p[0].abs() - 2.0).abs() < 1e-6 || p[0].abs() < 1e-6,
            "corner x is a box wall: {p:?}"
        );
        assert!(
            (p[1].abs() - 2.0).abs() < 1e-6 || p[1].abs() < 1e-6,
            "corner y is a box wall: {p:?}"
        );
    }
}

#[test]
fn contour_sources_are_side_faces_not_lids() {
    let r = cut(
        vec![object("box", "room-1", box_mesh([0.0, 0.0, 0.0], [2.0, 2.0, 2.0]))],
        [0.0, 0.0, 1.0],
        0.5,
    );
    let outers = closed_outer(&r);
    assert_eq!(outers.len(), 1);
    for f in &outers[0].face_refs {
        assert!(f.face_id <= 3, "only side faces 0..3 may source a horizontal section, got {}", f.face_id);
    }
    // All four walls must be represented.
    let ids: std::collections::BTreeSet<u32> = outers[0].face_refs.iter().map(|f| f.face_id).collect();
    assert_eq!(ids, [0, 1, 2, 3].into_iter().collect());
    // Every segment reports its own source.
    for seg in outers[0].segments.as_ref().unwrap() {
        assert!(!seg.sources.is_empty(), "every segment carries provenance");
    }
}

#[test]
fn hollow_volume_reports_hole_and_cap_excludes_it() {
    let objs = hollow_box_objects(
        "outer",
        "cavity",
        ([0.0, 0.0, 0.0], [2.0, 2.0, 2.0]),
        ([0.75, 0.75, 0.0], [1.25, 1.25, 2.0]),
    );
    let r = cut(objs, [0.0, 0.0, 1.0], 1.0);

    assert_eq!(r.stats.holes, 1, "inner cavity is a hole");
    assert_eq!(r.stats.closed_contours, 2);
    let outer = r.contours.iter().find(|c| c.kind == "outer").unwrap();
    let hole = r.contours.iter().find(|c| c.kind == "hole").unwrap();
    assert_close(outer.area_square_meters.unwrap(), 4.0, "outer area");
    assert_close(hole.area_square_meters.unwrap(), 0.25, "hole area");
    assert_eq!(hole.parent_id.as_deref(), Some(outer.id.as_str()));
    assert_eq!(hole.orientation.as_deref(), Some("cw"), "holes are CW");
    assert_eq!(outer.orientation.as_deref(), Some("ccw"));
    assert_eq!(r.caps.len(), 1);
    assert_close(r.caps[0].area_square_meters, 3.75, "cap subtracts the hole");
    assert_eq!(r.caps[0].hole_contour_ids, vec![hole.id.clone()]);
    // The hole contour's sources belong to the cavity object.
    assert_eq!(hole.object_ids, vec!["cavity".to_string()]);
}

#[test]
fn disconnected_components_are_never_joined() {
    let r = cut(
        vec![
            object("a", "room-a", box_mesh([0.0, 0.0, 0.0], [1.0, 1.0, 1.0])),
            object("b", "room-b", box_mesh([5.0, 0.0, 0.0], [6.0, 1.0, 1.0])),
        ],
        [0.0, 0.0, 1.0],
        0.5,
    );
    assert_eq!(closed_outer(&r).len(), 2, "two separate rooms stay separate");
    assert_eq!(r.caps.len(), 2);
    let total: f64 = closed_outer(&r).iter().map(|c| c.area_square_meters.unwrap()).sum();
    assert_close(total, 2.0, "summed area");
    for c in closed_outer(&r) {
        assert_eq!(c.object_ids.len(), 1, "each contour has a single owning object");
    }
}

#[test]
fn plane_exactly_through_vertical_edges_is_closed_and_correct() {
    // The plane x=y passes exactly through the two vertical edges of the cube
    // at (-1,-1,z) and (1,1,z). This is the degenerate "through edges" case.
    let r = cut(
        vec![object("cube", "room-1", box_mesh([-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]))],
        [1.0, -1.0, 0.0],
        0.0,
    );
    let outers = closed_outer(&r);
    assert_eq!(outers.len(), 1, "one closed rectangle from edge contacts");
    assert_close(outers[0].area_square_meters.unwrap(), 4.0 * 2f64.sqrt(), "diagonal section area");
    assert_eq!(outers[0].points2d.len(), 4);
    assert_eq!(r.caps.len(), 1);
    assert_eq!(r.stats.open_chains, 0);
    assert!(r.stats.crossing_edge_emitted >= 2, "edge contacts are emitted once: {}", r.stats.crossing_edge_emitted);
    // No segment may appear twice (duplicate short lines).
    let n = outers[0].segments.as_ref().unwrap().len();
    assert_eq!(n, 4, "four edges, no duplicates");
}

#[test]
fn plane_through_a_single_vertex_is_tangent_and_has_no_area() {
    // Normalised (1,1,1)/sqrt(3) touching only corner (1,1,1).
    let n = 1.0 / 3f64.sqrt();
    let r = cut(
        vec![object("cube", "room-1", box_mesh([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]))],
        [n, n, n],
        3.0 * n,
    );
    assert_eq!(r.contours.len(), 0, "tangent contact encloses no area");
    assert_eq!(r.caps.len(), 0);
    assert_eq!(r.stats.open_chains, 0);
    assert!(r.diagnostics.iter().any(|d| d.code == "tangentContact"));
    assert!(r.diagnostics.iter().any(|d| d.code == "emptySection"));
}

#[test]
fn plane_coplanar_with_a_lid_returns_that_face_outline() {
    let r = cut(
        vec![object("box", "room-1", box_mesh([0.0, 0.0, 0.0], [2.0, 2.0, 2.0]))],
        [0.0, 0.0, 1.0],
        2.0,
    );
    let outers = closed_outer(&r);
    assert_eq!(outers.len(), 1, "coplanar lid produces one outline");
    assert_close(outers[0].area_square_meters.unwrap(), 4.0, "lid area");
    assert!(!r.on_plane_face_refs.is_empty(), "coplanar faces are reported");
    assert!(r.stats.coplanar_triangles >= 2);
    assert_eq!(r.caps.len(), 1, "the coplanar face can still be capped");
    assert!(r.diagnostics.iter().any(|d| d.code == "coplanarFaces"));
}

#[test]
fn coincident_coplanar_faces_emit_one_outline() {
    // Two identical boxes occupying the same space: the duplicated lid at
    // z=2 must not produce two coincident outlines.
    let mesh = box_mesh([0.0, 0.0, 0.0], [2.0, 2.0, 2.0]);
    let r = cut(
        vec![
            object("dup-a", "room-1", mesh.clone()),
            object("dup-b", "room-1", mesh),
        ],
        [0.0, 0.0, 1.0],
        2.0,
    );
    // Per-object topology keeps the two shells distinct, but each contributes
    // exactly one rectangle; there are no extra segments within an object.
    for c in closed_outer(&r) {
        assert_eq!(c.points2d.len(), 4, "each duplicated face yields one clean ring");
    }
}

#[test]
fn thin_wall_section_has_correct_thickness() {
    // Wall 30 mm thick, 1 m long, 2.5 m tall, in a scene measured in mm.
    let wall = object(
        "wall",
        "room-1",
        box_mesh([0.0, 0.0, 0.0], [30.0, 1000.0, 2500.0]),
    );
    let mut s = scene(vec![wall]);
    s.unit = Some(section_kernel::model::UnitSpec {
        name: Some("millimeter".into()),
        meters_per_unit: Some(0.001),
    });
    let world = WorldScene::build(&s).unwrap();
    // Cut perpendicular to the wall length at its midpoint: the section is the
    // 30 mm x 2500 mm wall profile.
    let r = build_section(&req([0.0, 1.0, 0.0], 500.0), &world).unwrap();

    let outers = closed_outer(&r);
    assert_eq!(outers.len(), 1);
    assert_close(
        outers[0].area_square_meters.unwrap(),
        0.03 * 2.5,
        "thin wall area in m2",
    );
    assert_close(r.caps[0].area_square_meters, 0.03 * 2.5, "thin wall cap");
    let diagonal = (30f64.powi(2) + 1000f64.powi(2) + 2500f64.powi(2)).sqrt();
    assert_close(r.tolerance.linear, diagonal * 1e-7, "tolerance scales with model diagonal");
    assert_eq!(r.unit.name, "millimeter");
    assert_close(r.unit.meters_per_unit, 0.001, "unit scale reported");
}

#[test]
fn nonuniform_scale_is_applied_before_world_plane_test() {
    // Local cube 1x1x1 scaled to 4 x 2 x 1 in world space, then translated.
    // A naive implementation using the local vertices with a world plane
    // would place the section at the wrong height and area.
    let mut obj = object("scaled", "room-1", box_mesh([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]));
    obj.matrix = Some(mat4_from_rows([
        [4.0, 0.0, 0.0, 10.0],
        [0.0, 2.0, 0.0, 20.0],
        [0.0, 0.0, 1.0, 30.0],
        [0.0, 0.0, 0.0, 1.0],
    ]));
    let r = cut(vec![obj], [0.0, 0.0, 1.0], 30.5);

    let outers = closed_outer(&r);
    assert_eq!(outers.len(), 1);
    assert_close(outers[0].area_square_meters.unwrap(), 8.0, "scaled section area");
    // World-space corners must sit at x=10..14, y=20..22, z=30.5.
    for p in &outers[0].points3d {
        assert_close(p[2], 30.5, "world height");
        assert!(p[0] >= 10.0 - 1e-6 && p[0] <= 14.0 + 1e-6, "world x {p:?}");
        assert!(p[1] >= 20.0 - 1e-6 && p[1] <= 22.0 + 1e-6, "world y {p:?}");
    }
    // The section area in local units would be 1.0; only correct baking gives 8.
    assert!(outers[0].area_square_meters.unwrap() > 7.9);
}

#[test]
fn rotated_and_nonuniformly_scaled_section_is_area_correct() {
    // Local cube scaled (2,1,1) then rotated 45 degrees about Z. A horizontal
    // plane still cuts a 2 x 1 rectangle (rotation about Z preserves the
    // horizontal cross-section): area 2, but the world-side vertices are no
    // longer axis-aligned.
    let mut obj = object("rot", "room-1", box_mesh([-0.5, -0.5, -0.5], [0.5, 0.5, 0.5]));
    let rot = rotation_z_matrix(std::f64::consts::FRAC_PI_4);
    let scale = scale_matrix(2.0, 1.0, 1.0);
    // world = R * S (column-major multiply)
    let mut m = vec![0.0; 16];
    for c in 0..4 {
        for r in 0..4 {
            let mut sum = 0.0;
            for k in 0..4 {
                sum += rot[r + 4 * k] * scale[k + 4 * c];
            }
            m[r + 4 * c] = sum;
        }
    }
    obj.matrix = Some(m);
    let r = cut(vec![obj], [0.0, 0.0, 1.0], 0.0);
    let outers = closed_outer(&r);
    assert_eq!(outers.len(), 1);
    assert_close(outers[0].area_square_meters.unwrap(), 2.0, "rotated scaled area");
    // The rectangle is rotated 45 degrees: no corner is axis aligned at the
    // original 1.0/0.5 extents, which proves the transform was applied.
    let any_axis_aligned = outers[0]
        .points3d
        .iter()
        .any(|p| p[0].abs() - 1.0 < 1e-6 && p[1].abs() - 0.5 < 1e-6);
    assert!(!any_axis_aligned, "world vertices reflect the rotation");
}

#[test]
fn open_shell_keeps_open_diagnostics_and_fabricates_no_area() {
    // Remove the +X wall so a horizontal cut leaves a U-shaped open chain.
    let r = cut(
        vec![object("open", "room-1", open_box_mesh([0.0, 0.0, 0.0], [2.0, 2.0, 2.0]))],
        [0.0, 0.0, 1.0],
        0.5,
    );
    assert_eq!(r.stats.open_chains, 1, "gap remains an open chain");
    assert_eq!(r.stats.closed_contours, 0);
    assert_eq!(r.caps.len(), 0, "no cap invented for an open shell");
    let c = &r.contours[0];
    assert!(!c.closed);
    assert!(c.signed_area.is_none(), "open chain has no area");
    assert!(c.area_square_meters.is_none());
    assert!(r.diagnostics.iter().any(|d| d.code == "openChain"));
    assert!(r.diagnostics.iter().any(|d| d.code == "noClosedContours"));
    // The chain endpoints must be the two free ends: the open U has a
    // boundary of four corners in 3D (the missing wall leaves two short
    // returns at the cut), which is exactly what an uninvented gap looks like.
    assert_eq!(c.points2d.len(), 4, "open U boundary has four corners");
}

#[test]
fn result_is_deterministic_and_unit_metadata_is_explicit() {
    let objects = vec![object("box", "room-1", box_mesh([0.0, 0.0, 0.0], [2.0, 2.0, 2.0]))];
    let scene = scene(objects);
    let world = WorldScene::build(&scene).unwrap();
    let a = build_section(&req([0.0, 1.0, 0.0], 1.0), &world).unwrap();
    let b = build_section(&req([0.0, 1.0, 0.0], 1.0), &world).unwrap();
    assert_eq!(a.result_id, b.result_id);
    assert_eq!(a.plane.normal, [0.0, 1.0, 0.0]);
    assert_eq!(a.plane.offset, 1.0);
    assert!(a.plane.definition.contains("dot(normal, p)"));
    assert_eq!(a.unit.area_unit, "m2");
    assert!(a.coordinate_system.contains("+Z up"));
    assert!(!a.tolerance.rules.is_empty());
}

#[test]
fn plane_from_origin_point_is_equivalent_to_offset() {
    let objects = vec![object("box", "room-1", box_mesh([0.0, 0.0, 0.0], [2.0, 2.0, 2.0]))];
    let world = WorldScene::build(&scene(objects)).unwrap();
    let by_offset = build_section(&req([0.0, 0.0, 1.0], 0.5), &world).unwrap();
    let mut r = req([0.0, 0.0, 1.0], 0.0);
    r.offset = None;
    r.origin = Some([0.0, 0.0, 0.5]);
    let by_origin = build_section(&r, &world).unwrap();
    assert_close(by_origin.plane.offset, 0.5, "derived offset");
    assert_eq!(by_offset.result_id, by_origin.result_id);
}

#[test]
fn mirrored_transform_keeps_winding_and_section() {
    // Negative X scale mirrors the cube; winding must be restored and the
    // section must still report a positive, correctly oriented area.
    let mut obj = object("mirror", "room-1", box_mesh([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]));
    obj.matrix = Some(mat4_from_rows([
        [-1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]));
    let r = cut(vec![obj], [0.0, 0.0, 1.0], 0.5);
    let outers = closed_outer(&r);
    assert_eq!(outers.len(), 1);
    assert_close(outers[0].area_square_meters.unwrap(), 1.0, "mirrored area");
    assert_eq!(outers[0].orientation.as_deref(), Some("ccw"));
    assert!(r.objects[0].mirrored);
}

#[test]
fn invalid_scene_is_rejected_with_actionable_codes() {
    let mut broken = object("bad", "room-1", box_mesh([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]));
    broken.mesh.indices.push(999);
    let errs = WorldScene::build(&scene(vec![broken])).err().unwrap();
    assert!(errs.iter().any(|e| e.code == "invalid_indices"));
}

#[test]
fn section_points_are_exactly_on_the_plane() {
    let objects = vec![object("box", "room-1", box_mesh([0.0, 0.0, 0.0], [2.0, 2.0, 2.0]))];
    let world = WorldScene::build(&scene(objects)).unwrap();
    let n = [1.0f64, 2.0, 3.0];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    let plane_n = [n[0] / len, n[1] / len, n[2] / len];
    let offset = 0.7;
    let r = build_section(&req(n, offset), &world).unwrap();
    for c in &r.contours {
        for p in &c.points3d {
            let signed = plane_n[0] * p[0] + plane_n[1] * p[1] + plane_n[2] * p[2] - offset;
            assert!(signed.abs() <= r.tolerance.linear, "point off plane by {signed}");
        }
    }
    assert!(r.plane.basis_crosses_normal);
    assert_close(r.plane.offset, offset, "offset is preserved for a normalised normal");
}
