//! Emits the demo scene used by the web viewport and fixture tests.
//! Run: cargo run --example make_demo_scene -- <out.json>...

use serde_json::{json, Value};

fn box_mesh(min: [f64; 3], max: [f64; 3]) -> Value {
    let [x0, y0, z0] = min;
    let [x1, y1, z1] = max;
    let p = [
        [x0, y0, z0],
        [x1, y0, z0],
        [x0, y1, z0],
        [x1, y1, z0],
        [x0, y0, z1],
        [x1, y0, z1],
        [x0, y1, z1],
        [x1, y1, z1],
    ];
    // Faces: -X, +X, -Y, +Y, -Z, +Z (outward winding).
    let quads: [([usize; 4], u32); 6] = [
        ([0, 4, 6, 2], 0),
        ([1, 3, 7, 5], 1),
        ([0, 1, 5, 4], 2),
        ([2, 6, 7, 3], 3),
        ([0, 2, 3, 1], 4),
        ([4, 5, 7, 6], 5),
    ];
    let mut indices = Vec::new();
    let mut face_ids = Vec::new();
    for (q, fid) in quads {
        indices.extend_from_slice(&[q[0], q[1], q[2]]);
        face_ids.push(fid);
        indices.extend_from_slice(&[q[0], q[2], q[3]]);
        face_ids.push(fid);
    }
    json!({
        "positions": p.iter().flat_map(|v| v.iter().copied()).collect::<Vec<f64>>(),
        "indices": indices,
        "faceIds": face_ids,
    })
}

fn inward_box_mesh(min: [f64; 3], max: [f64; 3]) -> Value {
    let mut m = box_mesh(min, max);
    let idx = m["indices"].as_array_mut().unwrap();
    for chunk in idx.chunks_mut(3) {
        chunk.swap(1, 2);
    }
    m
}

fn object(
    id: &str,
    name: &str,
    group: &str,
    matrix: Option<Vec<f64>>,
    mesh: Value,
) -> Value {
    let mut o = json!({
        "id": id,
        "name": name,
        "group": group,
        "mesh": mesh,
    });
    if let Some(m) = matrix {
        o["matrix"] = json!(m);
    }
    o
}

fn mat_scale_translate(sx: f64, sy: f64, sz: f64, tx: f64, ty: f64, tz: f64) -> Vec<f64> {
    // Column-major.
    vec![
        sx, 0.0, 0.0, 0.0,
        0.0, sy, 0.0, 0.0,
        0.0, 0.0, sz, 0.0,
        tx, ty, tz, 1.0,
    ]
}

fn mat_rot_z_translate(rad: f64, tx: f64, ty: f64, tz: f64) -> Vec<f64> {
    let (s, c) = rad.sin_cos();
    // Column-major rotation about Z.
    vec![
        c, s, 0.0, 0.0,
        -s, c, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
        tx, ty, tz, 1.0,
    ]
}

fn main() {
    let dirs: Vec<String> = std::env::args().skip(1).collect();
    if dirs.is_empty() {
        eprintln!("usage: make_demo_scene <out.json>...  (stem selects the preset)");
        std::process::exit(2);
    }

    for path in dirs {
        let stem = std::path::Path::new(&path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let scene = match stem {
            "open-shell" => open_shell_scene(),
            _ => demo_building_scene(),
        };
        if let Some(parent) = std::path::Path::new(&path).parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::write(&path, serde_json::to_string_pretty(&scene).unwrap() + "\n")
            .unwrap_or_else(|e| panic!("write {path}: {e}"));
        println!("wrote {path}");
    }
}

/// A single room missing its +X wall. A horizontal cut leaves an open chain,
/// which the UI must report as diagnostic rather than a filled room.
fn open_shell_scene() -> Value {
    let mut mesh = box_mesh([0.0, 0.0, 0.0], [6.0, 4.0, 2.6]);
    let face_ids = mesh["faceIds"].as_array().unwrap().clone();
    let indices = mesh["indices"].as_array().unwrap().clone();
    let mut kept_indices = Vec::new();
    let mut kept_faces = Vec::new();
    for (i, f) in face_ids.iter().enumerate() {
        if f.as_u64() != Some(1) {
            kept_indices.extend_from_slice(&indices[i * 3..i * 3 + 3]);
            kept_faces.push(f.clone());
        }
    }
    mesh["indices"] = Value::Array(kept_indices);
    mesh["faceIds"] = Value::Array(kept_faces);

    json!({
        "schema": "archisection.scene/1",
        "name": "开放壳体（缺少 +X 墙）",
        "unit": { "name": "meter", "metersPerUnit": 1.0 },
        "objects": [
            object("open-room", "开放房间", "room-open", None, mesh),
            object("closed-box", "完整对照体", "room-closed", None,
                   box_mesh([9.0, 0.0, 0.0], [12.0, 3.0, 2.6])),
        ],
    })
}

fn demo_building_scene() -> Value {
    // A small two-room building with a hollow core, a thin partition created
    // from a uniformly-modelled wall scaled non-uniformly, a detached annex,
    // and a rotated tower. All coordinates are in metres. The transformed
    // objects verify that the kernel applies local-to-world matrices before
    // testing against the world-space cutting plane.
    let building_outer = ([0.0, 0.0, 0.0], [8.0, 6.0, 3.2]);
    let building_cavity = ([0.2, 0.2, 0.2], [7.8, 5.8, 3.0]);
    let annex_outer = ([11.0, 0.0, 0.0], [15.0, 4.0, 2.8]);
    let annex_cavity = ([11.2, 0.2, 0.2], [14.8, 3.8, 2.6]);

    let objects = vec![
        object(
            "building-outer",
            "主楼外壳",
            "building-a",
            None,
            box_mesh(building_outer.0, building_outer.1),
        ),
        object(
            "building-cavity",
            "主楼内腔",
            "building-a",
            None,
            inward_box_mesh(building_cavity.0, building_cavity.1),
        ),
        // Local wall is 1 x 6 x 3.2 m; non-uniform scale 0.12 makes it 120 mm
        // thick. The section must respect the scale, not read local vertices.
        object(
            "partition-wall",
            "薄隔墙（非均匀缩放 0.12）",
            "partition",
            Some(mat_scale_translate(0.12, 1.0, 1.0, 8.5, 0.0, 0.0)),
            box_mesh([0.0, 0.0, 0.0], [1.0, 6.0, 3.2]),
        ),
        object(
            "annex-outer",
            "附属房间外壳",
            "room-b",
            None,
            box_mesh(annex_outer.0, annex_outer.1),
        ),
        object(
            "annex-cavity",
            "附属房间内腔",
            "room-b",
            None,
            inward_box_mesh(annex_cavity.0, annex_cavity.1),
        ),
        object(
            "canopy",
            "雨篷板",
            "canopy",
            None,
            box_mesh([0.0, 7.0, 3.2], [8.0, 9.0, 3.35]),
        ),
        object(
            "tower",
            "旋转塔楼（绕 Z 30°）",
            "tower",
            Some(mat_rot_z_translate(30f64.to_radians(), 19.0, 8.0, 0.0)),
            box_mesh([-1.5, -1.5, 0.0], [1.5, 1.5, 6.0]),
        ),
    ];

    let scene = json!({
        "schema": "archisection.scene/1",
        "name": "演示建筑（两室带内腔 / 薄隔墙 / 旋转塔楼）",
        "unit": { "name": "meter", "metersPerUnit": 1.0 },
        "objects": objects,
    });

    scene
}
