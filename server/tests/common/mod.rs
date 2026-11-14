//! Shared geometry builders for the kernel tests and fixtures.
//! These are deliberately simple, explicit meshes so the tests can verify
//! intersection positions by hand.

use section_kernel::model::{Mat4, MeshDef, ObjectDef, Scene, UnitSpec};

pub fn tri_positions(verts: &[[f64; 3]]) -> Vec<f64> {
    verts.iter().flat_map(|v| v.iter().copied()).collect()
}

/// Axis-aligned box as 12 triangles, vertices at the eight corners.
/// `face_ids` gives a stable per-face id: 0..5 = -X,+X,-Y,+Y,-Z,+Z.
pub fn box_mesh(min: [f64; 3], max: [f64; 3]) -> MeshDef {
    let [x0, y0, z0] = min;
    let [x1, y1, z1] = max;
    // 8 corners: index bit 0 = x1, bit 1 = y1, bit 2 = z1.
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
    // Faces as quads, each split into two CCW triangles (outward winding).
    // (quad indices, face id)
    let quads: [([usize; 4], u32); 6] = [
        ([0, 4, 6, 2], 0), // -X
        ([1, 3, 7, 5], 1), // +X
        ([0, 1, 5, 4], 2), // -Y
        ([2, 6, 7, 3], 3), // +Y
        ([0, 2, 3, 1], 4), // -Z
        ([4, 5, 7, 6], 5), // +Z
    ];
    let mut indices = Vec::new();
    let mut face_ids = Vec::new();
    for (q, fid) in quads {
        indices.extend_from_slice(&[q[0] as u32, q[1] as u32, q[2] as u32]);
        face_ids.push(fid);
        indices.extend_from_slice(&[q[0] as u32, q[2] as u32, q[3] as u32]);
        face_ids.push(fid);
    }
    MeshDef {
        positions: tri_positions(&p),
        indices,
        face_ids: Some(face_ids),
    }
}

/// A box open on its +X side (that wall removed). A horizontal cut then
/// yields an open U-shaped chain rather than a closed rectangle.
pub fn open_box_mesh(min: [f64; 3], max: [f64; 3]) -> MeshDef {
    let mut m = box_mesh(min, max);
    // Drop triangles belonging to face id 1 (+X).
    let mut keep = Vec::new();
    let mut faces = Vec::new();
    for (i, f) in m.face_ids.clone().unwrap().iter().enumerate() {
        if *f != 1 {
            keep.extend_from_slice(&m.indices[i * 3..i * 3 + 3]);
            faces.push(*f);
        }
    }
    m.indices = keep;
    m.face_ids = Some(faces);
    m
}

/// A hollow box: outer shell plus an inward-wound inner shell, giving a
/// rectangular hole through the section when cut horizontally.
/// Returns two objects (outer and inner cavity) so that the cavity is a
/// separate component with its own winding.
pub fn hollow_box_objects(
    id_outer: &str,
    id_inner: &str,
    outer: ([f64; 3], [f64; 3]),
    inner: ([f64; 3], [f64; 3]),
) -> Vec<ObjectDef> {
    vec![
        ObjectDef {
            id: id_outer.to_string(),
            name: Some("outer shell".into()),
            group: Some("room-A".into()),
            room: Some("A".into()),
            matrix: None,
            mesh: box_mesh(outer.0, outer.1),
        },
        ObjectDef {
            id: id_inner.to_string(),
            name: Some("inner cavity".into()),
            group: Some("room-A".into()),
            room: Some("A".into()),
            matrix: None,
            mesh: inward_box_mesh(inner.0, inner.1),
        },
    ]
}

/// Box with reversed winding (normals point inward), representing a cavity
/// surface embedded inside a solid.
pub fn inward_box_mesh(min: [f64; 3], max: [f64; 3]) -> MeshDef {
    let mut m = box_mesh(min, max);
    for chunk in m.indices.chunks_mut(3) {
        chunk.swap(1, 2);
    }
    m
}

pub fn scene(objects: Vec<ObjectDef>) -> Scene {
    Scene {
        schema: Some("archisection.scene/1".into()),
        name: Some("test scene".into()),
        unit: Some(UnitSpec {
            name: Some("meter".into()),
            meters_per_unit: Some(1.0),
        }),
        objects,
    }
}

pub fn object(id: &str, group: &str, mesh: MeshDef) -> ObjectDef {
    ObjectDef {
        id: id.to_string(),
        name: Some(id.to_string()),
        group: Some(group.to_string()),
        room: None,
        matrix: None,
        mesh,
    }
}

/// Column-major matrix helpers for tests.
pub fn mat4_from_rows(rows: [[f64; 4]; 4]) -> Vec<f64> {
    let mut m = Vec::with_capacity(16);
    for c in 0..4 {
        for r in 0..4 {
            m.push(rows[r][c]);
        }
    }
    m
}

pub fn scale_matrix(sx: f64, sy: f64, sz: f64) -> Vec<f64> {
    let mut m = vec![0.0; 16];
    m[0] = sx;
    m[5] = sy;
    m[10] = sz;
    m[15] = 1.0;
    m
}

pub fn translation_matrix(tx: f64, ty: f64, tz: f64) -> Vec<f64> {
    let mut m = Mat4::identity().m.to_vec();
    m[12] = tx;
    m[13] = ty;
    m[14] = tz;
    m
}

pub fn rotation_y_matrix(radians: f64) -> Vec<f64> {
    let c = radians.cos();
    let s = radians.sin();
    let mut m = vec![0.0; 16];
    m[0] = c;
    m[2] = -s;
    m[5] = 1.0;
    m[8] = s;
    m[10] = c;
    m[15] = 1.0;
    m
}

pub fn rotation_z_matrix(radians: f64) -> Vec<f64> {
    let c = radians.cos();
    let s = radians.sin();
    let mut m = vec![0.0; 16];
    m[0] = c;
    m[1] = s;
    m[4] = -s;
    m[5] = c;
    m[10] = 1.0;
    m[15] = 1.0;
    m
}
