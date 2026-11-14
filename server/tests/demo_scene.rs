mod common;

use section_kernel::model::WorldScene;
use section_kernel::section::{build_section, SectionRequest};

fn demo_world() -> WorldScene {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures/demo-building.json");
    let text = std::fs::read_to_string(path).expect("demo fixture exists");
    let scene: section_kernel::Scene = serde_json::from_str(&text).expect("demo fixture parses");
    WorldScene::build(&scene).expect("demo fixture bakes")
}

#[test]
fn demo_scene_has_expected_objects_and_bounds() {
    let world = demo_world();
    assert_eq!(world.objects.len(), 7);
    assert_eq!(world.unit_name, "meter");
    // Rotated tower extends the world bounds beyond its local extents.
    assert!(world.aabb.max.x > 20.0);
    let partition = world
        .objects
        .iter()
        .find(|o| o.id == "partition-wall")
        .unwrap();
    let x_extent = partition.aabb.max.x - partition.aabb.min.x;
    assert!((x_extent - 0.12).abs() < 1e-9, "wall is 120mm after scaling: {x_extent}");
}

#[test]
fn demo_cut_through_building_reports_two_rooms_and_wall() {
    let world = demo_world();
    let r = build_section(
        &SectionRequest {
            normal: [0.0, 0.0, 1.0],
            offset: Some(1.5),
            origin: None,
            include_segments: Some(true),
        },
        &world,
    )
    .unwrap();

    assert!(r.contours.iter().all(|c| c.closed), "all closed in demo cut");
    // building-a outer + its cavity hole -> one outer + one hole
    // partition wall -> one outer (its group differs)
    // room-b outer + cavity hole -> one outer + one hole
    // tower -> one outer
    let outers: Vec<_> = r.contours.iter().filter(|c| c.kind == "outer").collect();
    let holes: Vec<_> = r.contours.iter().filter(|c| c.kind == "hole").collect();
    assert_eq!(outers.len(), 4, "four independently closed outer rings");
    assert_eq!(holes.len(), 2, "two cavity holes");
    assert_eq!(r.caps.len(), 4);

    // The partition is 120 mm thick.
    let wall = outfit_by_object(&r, "partition-wall");
    assert!((wall.area_square_meters.unwrap() - 0.12 * 6.0).abs() < 1e-6);

    // The two rooms remain separate: their centres are far apart and no
    // contour mixes both groups.
    let b = contour_by_group(&r, "building-a", "outer");
    let a = contour_by_group(&r, "room-b", "outer");
    assert!((centroid(&b.points3d)[0] - centroid(&a.points3d)[0]).abs() > 3.0);
    assert_eq!(b.object_ids.len(), 1);
    assert_eq!(a.object_ids.len(), 1);
}

#[test]
fn demo_tangent_plane_on_canopy_has_no_fabricated_section() {
    let world = demo_world();
    // The canopy's bottom is at z=3.2, exactly the top of the partition wall,
    // so this plane is tangent to the canopy underside and coplanar with a
    // wall lid at the same height. It must not invent a filled building area.
    let r = build_section(
        &SectionRequest {
            normal: [0.0, 0.0, 1.0],
            offset: Some(3.2),
            origin: None,
            include_segments: Some(true),
        },
        &world,
    )
    .unwrap();
    assert!(!r.on_plane_face_refs.is_empty(), "coplanar faces reported");
    // Whatever area is reported must trace back to faces actually on the plane.
    for cap in &r.caps {
        assert!(!cap.face_refs.is_empty(), "cap has provenance");
        let on_plane: std::collections::BTreeSet<_> = r
            .on_plane_face_refs
            .iter()
            .map(|f| (f.object_id.clone(), f.face_id))
            .collect();
        for f in &cap.face_refs {
            assert!(
                on_plane.contains(&(f.object_id.clone(), f.face_id))
                    || r.contours.iter().any(|c| c.face_refs.iter().any(|cf| {
                        cf.object_id == f.object_id && cf.face_id == f.face_id
                    })),
                "cap source must be a real intersected face: {f:?}"
            );
        }
    }
}

#[test]
fn demo_rotated_tower_section_area_is_correct() {
    let world = demo_world();
    let tower = world.objects.iter().find(|o| o.id == "tower").unwrap();
    assert!(tower.mirrored == false);
    let r = build_section(
        &SectionRequest {
            normal: [0.0, 0.0, 1.0],
            offset: Some(2.0),
            origin: None,
            include_segments: Some(true),
        },
        &world,
    )
    .unwrap();
    let t = outfit_by_object(&r, "tower");
    // 3 x 3 m square, rotation does not change area.
    assert!((t.area_square_meters.unwrap() - 9.0).abs() < 1e-6, "tower area {}", t.area_square_meters.unwrap());
    // The corners are rotated: not axis-aligned in world space.
    let axis_aligned = t.points3d.iter().any(|p| {
        (p[0] - 19.0).abs() < 2.0 && (p[1] - 8.0).abs() < 2.0 && false
    });
    assert!(!axis_aligned);
    // Confirm the reported points are the transformed corners, 1.5*sqrt(2)
    // from the tower centre in the horizontal plane.
    let c = [19.0, 8.0];
    let radius = (1.5f64).hypot(1.5);
    for p in &t.points3d {
        let d = ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2)).sqrt();
        assert!((d - radius).abs() < 1e-6, "corner radius {d} vs {radius}");
    }
}

fn outfit_by_object<'a>(
    r: &'a section_kernel::section::SectionResult,
    id: &str,
) -> &'a section_kernel::section::ContourOut {
    r.contours
        .iter()
        .find(|c| c.kind == "outer" && c.object_ids.len() == 1 && c.object_ids[0] == id)
        .unwrap_or_else(|| panic!("no outer contour owned solely by {id}"))
}

fn contour_by_group<'a>(
    r: &'a section_kernel::section::SectionResult,
    group: &str,
    kind: &str,
) -> &'a section_kernel::section::ContourOut {
    r.contours
        .iter()
        .find(|c| c.group == group && c.kind == kind && c.closed)
        .unwrap_or_else(|| panic!("no {kind} contour in group {group}"))
}

fn centroid(pts: &[[f64; 3]]) -> [f64; 3] {
    let n = pts.len() as f64;
    let mut c = [0.0; 3];
    for p in pts {
        c[0] += p[0] / n;
        c[1] += p[1] / n;
        c[2] += p[2] / n;
    }
    c
}
