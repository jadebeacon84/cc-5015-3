use crate::math::{Aabb, Vec3};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize)]
pub struct Scene {
    #[serde(default)]
    pub schema: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub unit: Option<UnitSpec>,
    pub objects: Vec<ObjectDef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UnitSpec {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(rename = "metersPerUnit", default)]
    pub meters_per_unit: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ObjectDef {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    /// Contour/cap grouping key (room, storey, assembly). Defaults to the object id.
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub room: Option<String>,
    /// Column-major 4x4 local-to-world matrix. Omitted means identity.
    #[serde(default)]
    pub matrix: Option<Vec<f64>>,
    pub mesh: MeshDef,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MeshDef {
    pub positions: Vec<f64>,
    pub indices: Vec<u32>,
    /// Optional source-face id per triangle (triangulated face -> original face).
    #[serde(rename = "faceIds", default)]
    pub face_ids: Option<Vec<u32>>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Mat4 {
    /// Column-major.
    pub m: [f64; 16],
}

impl Mat4 {
    pub fn identity() -> Self {
        let mut m = [0.0; 16];
        m[0] = 1.0;
        m[5] = 1.0;
        m[10] = 1.0;
        m[15] = 1.0;
        Self { m }
    }
    pub fn from_slice(v: &[f64]) -> Option<Self> {
        if v.len() != 16 {
            return None;
        }
        let mut m = [0.0; 16];
        m.copy_from_slice(v);
        if !m.iter().all(|x| x.is_finite()) {
            return None;
        }
        Some(Self { m })
    }
    /// Only affine transforms are accepted: the bottom row must be [0,0,0,1].
    /// Otherwise the world mapping would introduce perspective and the
    /// section plane would no longer be a plane in the transformed mesh.
    pub fn is_affine(&self) -> bool {
        let m = &self.m;
        m[3].abs() <= 1e-12 && m[7].abs() <= 1e-12 && m[11].abs() <= 1e-12 && (m[15] - 1.0).abs() <= 1e-12
    }
    pub fn transform_point(&self, p: Vec3) -> Vec3 {
        let m = &self.m;
        Vec3::new(
            m[0] * p.x + m[4] * p.y + m[8] * p.z + m[12],
            m[1] * p.x + m[5] * p.y + m[9] * p.z + m[13],
            m[2] * p.x + m[6] * p.y + m[10] * p.z + m[14],
        )
    }
    pub fn determinant(&self) -> f64 {
        let m = &self.m;
        m[0] * (m[5] * m[10] - m[6] * m[9])
            - m[4] * (m[1] * m[10] - m[2] * m[9])
            + m[8] * (m[1] * m[6] - m[2] * m[5])
    }
}

#[derive(Debug, Clone)]
pub struct Triangle {
    pub v: [u32; 3],
    pub face_id: u32,
}

#[derive(Debug, Clone)]
pub struct WorldObject {
    pub id: String,
    pub name: String,
    pub group: String,
    pub room: Option<String>,
    pub positions: Vec<Vec3>,
    pub triangles: Vec<Triangle>,
    pub matrix: Mat4,
    pub mirrored: bool,
    pub aabb: Aabb,
}

#[derive(Debug, Clone)]
pub struct WorldScene {
    pub name: String,
    pub unit_name: String,
    pub meters_per_unit: f64,
    pub objects: Vec<WorldObject>,
    pub aabb: Aabb,
    /// Number of duplicate/short triangle declarations removed during baking.
    pub bake_warnings: BTreeMap<String, usize>,
    pub rejected_objects: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SceneError {
    pub code: &'static str,
    pub message: String,
    pub object_id: Option<String>,
}

impl SceneError {
    fn new(code: &'static str, message: impl Into<String>, object_id: Option<&str>) -> Self {
        Self {
            code,
            message: message.into(),
            object_id: object_id.map(|s| s.to_string()),
        }
    }
}

impl WorldScene {
    pub fn build(scene: &Scene) -> Result<WorldScene, Vec<SceneError>> {
        let mut errs = Vec::new();
        if scene.objects.is_empty() {
            errs.push(SceneError::new("empty_scene", "scene contains no objects", None));
            return Err(errs);
        }
        let (unit_name, mpu) = match &scene.unit {
            Some(u) => {
                let mpu = u.meters_per_unit.unwrap_or(1.0);
                if !mpu.is_finite() || mpu <= 0.0 {
                    errs.push(SceneError::new(
                        "invalid_unit",
                        "metersPerUnit must be a positive finite number",
                        None,
                    ));
                    (u.name.clone().unwrap_or_else(|| "unit".into()), 1.0)
                } else {
                    (u.name.clone().unwrap_or_else(|| "unit".into()), mpu)
                }
            }
            None => ("unit".to_string(), 1.0),
        };

        let mut objects = Vec::with_capacity(scene.objects.len());
        let mut warnings: BTreeMap<String, usize> = BTreeMap::new();
        let mut rejected = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();

        for def in &scene.objects {
            if def.id.trim().is_empty() {
                errs.push(SceneError::new("missing_object_id", "object id is empty", None));
                continue;
            }
            if !seen_ids.insert(def.id.clone()) {
                errs.push(SceneError::new(
                    "duplicate_object_id",
                    format!("duplicate object id '{}'", def.id),
                    Some(&def.id),
                ));
                continue;
            }
            match bake_object(def) {
                Ok((obj, notes)) => {
                    for n in notes {
                        *warnings.entry(n.to_string()).or_insert(0) += 1;
                    }
                    objects.push(obj);
                }
                Err(e) => {
                    rejected.push(def.id.clone());
                    errs.push(e);
                }
            }
        }

        if objects.is_empty() {
            return Err(errs);
        }
        if !errs.is_empty() {
            return Err(errs);
        }

        let mut aabb = Aabb::empty();
        for o in &objects {
            if o.aabb.valid() {
                aabb.add(o.aabb.min);
                aabb.add(o.aabb.max);
            }
        }
        if !aabb.valid() {
            return Err(vec![SceneError::new(
                "degenerate_scene",
                "scene has no finite geometry",
                None,
            )]);
        }

        Ok(WorldScene {
            name: scene.name.clone().unwrap_or_else(|| "untitled".into()),
            unit_name,
            meters_per_unit: mpu,
            objects,
            aabb,
            bake_warnings: warnings,
            rejected_objects: rejected,
        })
    }
}

fn bake_object(def: &ObjectDef) -> Result<(WorldObject, Vec<&'static str>), SceneError> {
    let id = def.id.as_str();
    let m = match &def.matrix {
        Some(v) => Mat4::from_slice(v).ok_or_else(|| {
            SceneError::new(
                "invalid_matrix",
                "matrix must contain 16 finite column-major values",
                Some(id),
            )
        })?,
        None => Mat4::identity(),
    };

    let p = &def.mesh.positions;
    if p.len() % 3 != 0 || p.is_empty() {
        return Err(SceneError::new(
            "invalid_positions",
            "positions must be a non-empty xyz array",
            Some(id),
        ));
    }
    if def.mesh.indices.len() % 3 != 0 || def.mesh.indices.is_empty() {
        return Err(SceneError::new(
            "invalid_indices",
            "indices must be a non-empty triangle index array",
            Some(id),
        ));
    }
    let vertex_count = p.len() / 3;
    let tri_count = def.mesh.indices.len() / 3;
    if let Some(face_ids) = &def.mesh.face_ids {
        if face_ids.len() != tri_count {
            return Err(SceneError::new(
                "invalid_face_ids",
                format!(
                    "faceIds length {} does not match triangle count {}",
                    face_ids.len(),
                    tri_count
                ),
                Some(id),
            ));
        }
    }

    let det = m.determinant();
    if !det.is_finite() || det.abs() < 1e-15 {
        return Err(SceneError::new(
            "singular_matrix",
            "local-to-world matrix is singular (zero scale or collapsed axis)",
            Some(id),
        ));
    }
    if !m.is_affine() {
        return Err(SceneError::new(
            "non_affine_matrix",
            "local-to-world matrix must be affine (bottom row 0,0,0,1)",
            Some(id),
        ));
    }
    // Explicitly reject a scale axis that is too close to zero after
    // decomposition, which a determinant check alone can miss for
    // non-uniform transforms.
    let axis_x = Vec3::new(m.m[0], m.m[1], m.m[2]).length();
    let axis_y = Vec3::new(m.m[4], m.m[5], m.m[6]).length();
    let axis_z = Vec3::new(m.m[8], m.m[9], m.m[10]).length();
    if axis_x < 1e-9 || axis_y < 1e-9 || axis_z < 1e-9 {
        return Err(SceneError::new(
            "collapsed_axis",
            "local-to-world matrix has a collapsed basis axis",
            Some(id),
        ));
    }
    let mirrored = det < 0.0;

    let mut positions = Vec::with_capacity(vertex_count);
    let mut aabb = Aabb::empty();
    for i in 0..vertex_count {
        let local = Vec3::new(p[i * 3], p[i * 3 + 1], p[i * 3 + 2]);
        if !local.is_finite() {
            return Err(SceneError::new(
                "non_finite_position",
                "mesh contains a non-finite position",
                Some(id),
            ));
        }
        let world = m.transform_point(local);
        aabb.add(world);
        positions.push(world);
    }

    let mut triangles = Vec::with_capacity(tri_count);
    let mut notes = Vec::new();
    for t in 0..tri_count {
        let i0 = def.mesh.indices[t * 3];
        let i1 = def.mesh.indices[t * 3 + 1];
        let i2 = def.mesh.indices[t * 3 + 2];
        if i0 as usize >= vertex_count || i1 as usize >= vertex_count || i2 as usize >= vertex_count {
            return Err(SceneError::new(
                "index_out_of_range",
                format!("triangle {} references a vertex outside 0..{}", t, vertex_count),
                Some(id),
            ));
        }
        // Mirroring a transform flips winding; restore outward orientation so
        // side classification stays consistent with the plane normal.
        let (a, b, c) = if mirrored { (i0, i2, i1) } else { (i0, i1, i2) };
        if a == b || b == c || a == c {
            notes.push("degenerate_triangle_skipped");
            continue;
        }
        // Catches geometry that collapsed under a non-uniform / singular-ish scale.
        let area2 = positions[b as usize]
            .sub(positions[a as usize])
            .cross(positions[c as usize].sub(positions[a as usize]))
            .length();
        if area2 <= 1e-18 {
            notes.push("degenerate_triangle_skipped");
            continue;
        }
        let face_id = match &def.mesh.face_ids {
            Some(ids) => ids[t],
            None => t as u32,
        };
        triangles.push(Triangle {
            v: [a, b, c],
            face_id,
        });
    }
    if triangles.is_empty() {
        return Err(SceneError::new(
            "no_triangles",
            "object contains no non-degenerate triangles",
            Some(id),
        ));
    }

    let group = def
        .group
        .clone()
        .or_else(|| def.room.clone())
        .unwrap_or_else(|| def.id.clone());

    Ok((
        WorldObject {
            id: def.id.clone(),
            name: def.name.clone().unwrap_or_else(|| def.id.clone()),
            group,
            room: def.room.clone(),
            positions,
            triangles,
            matrix: m,
            mirrored,
            aabb,
        },
        notes,
    ))
}
