use crate::math::{Plane, Tolerance, Vec3};
use crate::model::{SceneError, WorldScene};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

// ---------------------------------------------------------------------------
// Public request / response schema
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionRequest {
    /// Plane normal in world/model coordinates. Need not be unit length.
    pub normal: [f64; 3],
    /// Signed offset d such that dot(normal, p) = offset is on the plane.
    #[serde(default)]
    pub offset: Option<f64>,
    /// Alternative to offset: a point lying on the plane.
    #[serde(default)]
    pub origin: Option<[f64; 3]>,
    #[serde(default)]
    pub include_segments: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaneOut {
    pub normal: [f64; 3],
    pub offset: f64,
    pub origin: [f64; 3],
    pub basis_u: [f64; 3],
    pub basis_v: [f64; 3],
    pub basis_crosses_normal: bool,
    pub definition: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitOut {
    pub name: String,
    pub meters_per_unit: f64,
    pub area_unit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundsOut {
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub diagonal: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToleranceOut {
    pub linear: f64,
    pub angular: f64,
    pub rules: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FaceRef {
    pub object_id: String,
    pub face_id: u32,
    pub triangle_index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentOut {
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub a3d: [f64; 3],
    pub b3d: [f64; 3],
    pub sources: Vec<FaceRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContourOut {
    pub id: String,
    pub group: String,
    pub kind: String,
    pub closed: bool,
    pub parent_id: Option<String>,
    pub depth: usize,
    pub orientation: Option<String>,
    /// Ring in the plane's (u,v) basis. Closed rings do not repeat the first point.
    pub points2d: Vec<[f64; 2]>,
    /// Same ring projected back to world coordinates (exactly on the plane).
    pub points3d: Vec<[f64; 3]>,
    pub signed_area: Option<f64>,
    pub area_square_meters: Option<f64>,
    pub object_ids: Vec<String>,
    pub face_refs: Vec<FaceRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segments: Option<Vec<SegmentOut>>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapOut {
    pub id: String,
    pub group: String,
    pub outer_contour_id: String,
    pub hole_contour_ids: Vec<String>,
    /// World-space triangles, all three vertices on the plane.
    pub triangles: Vec<[[f64; 3]; 3]>,
    pub area: f64,
    pub area_square_meters: f64,
    pub normal: [f64; 3],
    pub object_ids: Vec<String>,
    pub face_refs: Vec<FaceRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub code: String,
    pub severity: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contour_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsOut {
    pub input_triangles: usize,
    pub section_triangles: usize,
    pub segments: usize,
    pub nodes: usize,
    pub closed_contours: usize,
    pub open_chains: usize,
    pub holes: usize,
    pub caps: usize,
    pub coplanar_triangles: usize,
    pub on_plane_vertices: usize,
    pub dropped_short_segments: usize,
    pub merged_duplicate_segments: usize,
    pub tangent_triangles: usize,
    pub crossing_edge_emitted: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionResult {
    pub schema: String,
    pub result_id: String,
    pub scene_name: String,
    pub unit: UnitOut,
    pub coordinate_system: String,
    pub bounds: BoundsOut,
    pub plane: PlaneOut,
    pub tolerance: ToleranceOut,
    pub contours: Vec<ContourOut>,
    pub caps: Vec<CapOut>,
    pub diagnostics: Vec<Diagnostic>,
    pub stats: StatsOut,
    pub objects: Vec<ObjectOut>,
    /// Triangles lying in the plane's zero-set (not boundary crossings).
    pub on_plane_face_refs: Vec<FaceRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectOut {
    pub id: String,
    pub name: String,
    pub group: String,
    pub triangles: usize,
    pub mirrored: bool,
}

// ---------------------------------------------------------------------------
// Internal working types
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TriClass {
    Same,
    Graze,
    Crossing,
    Coplanar,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum PointKey {
    Vertex(u32),
    Edge(u32, u32),
}

/// Topological identity is scoped per object; two objects may use the same
/// vertex index for entirely different points.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct NodeKey {
    obj: usize,
    point: PointKey,
}

#[derive(Clone)]
struct Source {
    object_id: String,
    face_id: u32,
    triangle_index: usize,
    group: String,
}

struct Node {
    pos3: Vec3,
    pos2: [f64; 2],
}

struct Segment {
    a: usize,
    b: usize,
    sources: Vec<Source>,
    origin: SegOrigin,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SegOrigin {
    Crossing,
    CoplanarPatch,
    GrazeEdge,
}

struct LoopBuild {
    group: String,
    closed: bool,
    node_ids: Vec<usize>,
    edge_ids: Vec<usize>,
    sources: Vec<Source>,
    diagnostics: Vec<String>,
}

struct ObjectFrame {
    sd: Vec<f64>,
    tris: Vec<TriClass>,
    sources: Vec<Source>,
}

struct NodeStore {
    nodes: Vec<Node>,
    key_map: HashMap<NodeKey, usize>,
    grid: HashMap<(i64, i64), Vec<usize>>,
    tol: f64,
    inv_cell: f64,
}

impl NodeStore {
    fn new(tol: f64) -> Self {
        Self {
            nodes: Vec::new(),
            key_map: HashMap::new(),
            grid: HashMap::new(),
            tol,
            inv_cell: 1.0 / (tol * 3.0).max(1e-12),
        }
    }

    fn get_or_insert(&mut self, key: NodeKey, pos3: Vec3, pos2: [f64; 2]) -> usize {
        if let Some(&i) = self.key_map.get(&key) {
            return i;
        }
        let g = (
            (pos2[0] * self.inv_cell).floor() as i64,
            (pos2[1] * self.inv_cell).floor() as i64,
        );
        for dx in -1i64..=1 {
            for dy in -1i64..=1 {
                if let Some(list) = self.grid.get(&(g.0 + dx, g.1 + dy)) {
                    for &i in list {
                        let ddx = self.nodes[i].pos2[0] - pos2[0];
                        let ddy = self.nodes[i].pos2[1] - pos2[1];
                        if ddx.abs() <= self.tol && ddy.abs() <= self.tol {
                            self.key_map.insert(key, i);
                            return i;
                        }
                    }
                }
            }
        }
        let id = self.nodes.len();
        self.nodes.push(Node { pos3, pos2 });
        self.grid.entry(g).or_default().push(id);
        self.key_map.insert(key, id);
        id
    }
}

struct SegmentStore {
    segs: Vec<Segment>,
    by_pair: HashMap<(usize, usize), usize>,
    merged: usize,
}

impl SegmentStore {
    fn new() -> Self {
        Self {
            segs: Vec::new(),
            by_pair: HashMap::new(),
            merged: 0,
        }
    }
    fn push(&mut self, ia: usize, ib: usize, src: &Source, origin: SegOrigin) -> Option<usize> {
        if ia == ib {
            return None;
        }
        let key = (ia.min(ib), ia.max(ib));
        if let Some(&si) = self.by_pair.get(&key) {
            self.merged += 1;
            let seg = &mut self.segs[si];
            if !seg.sources.iter().any(|s| {
                s.object_id == src.object_id
                    && s.face_id == src.face_id
                    && s.triangle_index == src.triangle_index
            }) {
                seg.sources.push(src.clone());
            }
            // A crossing interpretation wins over a bounding/coplanar one: the
            // edge genuinely passes through the solid.
            if origin == SegOrigin::Crossing {
                seg.origin = SegOrigin::Crossing;
            }
            return Some(si);
        }
        self.by_pair.insert(key, self.segs.len());
        self.segs.push(Segment {
            a: key.0,
            b: key.1,
            sources: vec![src.clone()],
            origin,
        });
        Some(self.segs.len() - 1)
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn build_section(req: &SectionRequest, scene: &WorldScene) -> Result<SectionResult, Vec<SceneError>> {
    let n_raw = Vec3::new(req.normal[0], req.normal[1], req.normal[2]);
    if !n_raw.is_finite() || n_raw.length() < 1e-12 {
        return Err(vec![SceneError {
            code: "invalid_plane".into(),
            message: "plane normal must be a finite, non-zero vector".into(),
            object_id: None,
        }]);
    }
    let n = n_raw.normalize();
    let d = match req.origin {
        Some(origin) => {
            let o = Vec3::new(origin[0], origin[1], origin[2]);
            if !o.is_finite() {
                return Err(vec![SceneError {
                    code: "invalid_plane".into(),
                    message: "plane origin must be finite".into(),
                    object_id: None,
                }]);
            }
            n.dot(o)
        }
        None => match req.offset {
            Some(d) if d.is_finite() => d,
            _ => {
                return Err(vec![SceneError {
                    code: "invalid_plane".into(),
                    message: "plane offset (or origin) is required and must be finite".into(),
                    object_id: None,
                }])
            }
        },
    };
    let plane = Plane { n, d };
    let (u, v) = plane.basis();
    let tol = Tolerance::from_extents(scene.aabb.diagonal(), 1.0);
    if tol.linear <= 0.0 {
        return Err(vec![SceneError {
            code: "degenerate_scene".into(),
            message: "model extents are too small to define a tolerance".into(),
            object_id: None,
        }]);
    }
    let include_segments = req.include_segments.unwrap_or(true);
    let origin = plane.origin();
    let to2d = |p: Vec3| -> [f64; 2] {
        let rel = p.sub(origin);
        [rel.dot(u), rel.dot(v)]
    };

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut on_plane_face_refs: Vec<FaceRef> = Vec::new();
    let mut on_plane_vertices = 0usize;
    let mut tangent_triangles = 0usize;
    let mut frames: Vec<ObjectFrame> = Vec::with_capacity(scene.objects.len());

    for obj in scene.objects.iter() {
        let mut sd: Vec<f64> = Vec::with_capacity(obj.positions.len());
        for p in &obj.positions {
            sd.push(plane.signed_distance(*p));
        }
        on_plane_vertices += sd.iter().filter(|x| x.abs() <= tol.linear).count();
        let mut tris = Vec::with_capacity(obj.triangles.len());
        let mut sources = Vec::with_capacity(obj.triangles.len());
        for (ti, t) in obj.triangles.iter().enumerate() {
            let s = [
                classify(sd[t.v[0] as usize], tol.linear),
                classify(sd[t.v[1] as usize], tol.linear),
                classify(sd[t.v[2] as usize], tol.linear),
            ];
            let has_on = s.iter().any(|x| *x == 0);
            let has_pos = s.iter().any(|x| *x > 0);
            let has_neg = s.iter().any(|x| *x < 0);
            let class = if has_on && !has_pos && !has_neg {
                TriClass::Coplanar
            } else if has_pos && has_neg {
                TriClass::Crossing
            } else if has_on {
                TriClass::Graze
            } else {
                TriClass::Same
            };
            if class == TriClass::Coplanar {
                on_plane_face_refs.push(FaceRef {
                    object_id: obj.id.clone(),
                    face_id: t.face_id,
                    triangle_index: ti,
                });
            }
            if matches!(class, TriClass::Graze | TriClass::Coplanar) {
                tangent_triangles += 1;
            }
            tris.push(class);
            sources.push(Source {
                object_id: obj.id.clone(),
                face_id: t.face_id,
                triangle_index: ti,
                group: obj.group.clone(),
            });
        }
        frames.push(ObjectFrame { sd, tris, sources });
    }

    // --- topology-canonical nodes and segments -------------------------
    let mut nodes = NodeStore::new(tol.linear);
    let mut segs = SegmentStore::new();
    let mut dropped_short_segments = 0usize;
    let mut section_triangles = 0usize;
    let mut crossing_edge_emitted = 0usize;

    // Coplanar triangles: classify each edge by the side of its third vertex.
    // Deduplicating users by third-vertex position makes coincident duplicate
    // faces collapse to one outline, while genuine adjacent patches cancel.
    struct CoplanarEdge {
        users: BTreeSet<(i8, i64, i64)>,
        source: Source,
    }
    let mut coplanar_edges: BTreeMap<(usize, u32, u32), CoplanarEdge> = BTreeMap::new();

    // Grazing triangles whose plane contact is a full mesh edge. The surface
    // crosses the plane along such an edge only when triangles exist on both
    // sides of it; otherwise the edge is tangency.
    struct GrazeEdge {
        pos: Option<Source>,
        neg: Option<Source>,
    }
    let mut graze_edges: BTreeMap<(usize, u32, u32), GrazeEdge> = BTreeMap::new();

    for (oi, obj) in scene.objects.iter().enumerate() {
        let frame = &frames[oi];
        for (ti, t) in obj.triangles.iter().enumerate() {
            let src = &frame.sources[ti];
            let s = [
                classify(frame.sd[t.v[0] as usize], tol.linear),
                classify(frame.sd[t.v[1] as usize], tol.linear),
                classify(frame.sd[t.v[2] as usize], tol.linear),
            ];
            match frame.tris[ti] {
                TriClass::Same => {}
                TriClass::Coplanar => {
                    // For each edge, find the third vertex and its 2D side.
                    for (ea, eb, ec) in [
                        (t.v[0], t.v[1], t.v[2]),
                        (t.v[1], t.v[2], t.v[0]),
                        (t.v[2], t.v[0], t.v[1]),
                    ] {
                        let (ka, kb) = canon_edge(ea, eb);
                        let pa = to2d(plane.project(obj.positions[ka as usize]));
                        let pb = to2d(plane.project(obj.positions[kb as usize]));
                        let pc = to2d(plane.project(obj.positions[ec as usize]));
                        let side = cross2([pb[0] - pa[0], pb[1] - pa[1]], [pc[0] - pa[0], pc[1] - pa[1]]);
                        let sign = if side > 0.0 {
                            1i8
                        } else if side < 0.0 {
                            -1i8
                        } else {
                            0i8
                        };
                        let q = |x: f64| (x / (tol.linear * 3.0).max(1e-12)).round() as i64;
                        let entry = coplanar_edges.entry((oi, ka, kb)).or_insert(CoplanarEdge {
                            users: BTreeSet::new(),
                            source: src.clone(),
                        });
                        entry.users.insert((sign, q(pc[0]), q(pc[1])));
                    }
                }
                TriClass::Graze => {
                    let on: Vec<u32> = t
                        .v
                        .iter()
                        .copied()
                        .filter(|&vi| s[if t.v[0] == vi { 0 } else if t.v[1] == vi { 1 } else { 2 }] == 0)
                        .collect();
                    if on.len() == 2 {
                        let off = t
                            .v
                            .iter()
                            .copied()
                            .find(|&vi| {
                                s[if t.v[0] == vi { 0 } else if t.v[1] == vi { 1 } else { 2 }] != 0
                            })
                            .unwrap();
                        let off_sign = s[if t.v[0] == off {
                            0
                        } else if t.v[1] == off {
                            1
                        } else {
                            2
                        }];
                        let (ka, kb) = canon_edge(on[0], on[1]);
                        let e = graze_edges.entry((oi, ka, kb)).or_insert(GrazeEdge {
                            pos: None,
                            neg: None,
                        });
                        if off_sign > 0 {
                            if e.pos.is_none() {
                                e.pos = Some(src.clone());
                            }
                        } else if e.neg.is_none() {
                            e.neg = Some(src.clone());
                        }
                    }
                }
                TriClass::Crossing => {
                    let mut cand: Vec<(PointKey, Vec3, [f64; 2])> = Vec::new();
                    for (ea, eb) in [(t.v[0], t.v[1]), (t.v[1], t.v[2]), (t.v[2], t.v[0])] {
                        let (ka, kb) = canon_edge(ea, eb);
                        let sa = s[tri_pos(t.v, ea)];
                        let sb = s[tri_pos(t.v, eb)];
                        let key = if sa == 0 {
                            Some(PointKey::Vertex(ka))
                        } else if sb == 0 {
                            Some(PointKey::Vertex(kb))
                        } else if (sa > 0) != (sb > 0) {
                            Some(PointKey::Edge(ka, kb))
                        } else {
                            None
                        };
                        let Some(key) = key else { continue };
                        if cand.iter().any(|(k, _, _)| *k == key) {
                            continue;
                        }
                        let pos3 = match key {
                            PointKey::Vertex(vi) => plane.project(obj.positions[vi as usize]),
                            PointKey::Edge(_, _) => {
                                // Sorted endpoints make the interpolation
                                // bit-identical for the two adjacent
                                // crossing triangles.
                                let s_ka = s[tri_pos(t.v, ka)];
                                let s_kb = s[tri_pos(t.v, kb)];
                                let frac = (s_ka as f64) / ((s_ka - s_kb) as f64);
                                plane.project(
                                    obj.positions[ka as usize]
                                        .lerp(obj.positions[kb as usize], frac),
                                )
                            }
                        };
                        cand.push((key, pos3, to2d(pos3)));
                    }
                    if cand.len() < 2 {
                        continue;
                    }
                    let pair = farthest_pair(&cand);
                    if cand[pair.0].1.sub(cand[pair.1].1).length() <= tol.linear * 4.0 {
                        dropped_short_segments += 1;
                        continue;
                    }
                    section_triangles += 1;
                    let ia = nodes.get_or_insert(
                        NodeKey {
                            obj: oi,
                            point: cand[pair.0].0,
                        },
                        cand[pair.0].1,
                        cand[pair.0].2,
                    );
                    let ib = nodes.get_or_insert(
                        NodeKey {
                            obj: oi,
                            point: cand[pair.1].0,
                        },
                        cand[pair.1].1,
                        cand[pair.1].2,
                    );
                    segs.push(ia, ib, src, SegOrigin::Crossing);
                }
            }
        }
    }

    // Coplanar patch boundary: users on both sides mean an interior edge of a
    // triangulated patch (or two coincident sheets cancel); otherwise the edge
    // is on the patch boundary and is emitted once.
    for ((oi, ka, kb), edge) in coplanar_edges.iter() {
        let has_pos = edge.users.iter().any(|(s, _, _)| *s >= 0);
        let has_neg = edge.users.iter().any(|(s, _, _)| *s <= 0);
        if has_pos && has_neg && edge.users.len() >= 2 {
            continue;
        }
        let obj = &scene.objects[*oi];
        let pa = plane.project(obj.positions[*ka as usize]);
        let pb = plane.project(obj.positions[*kb as usize]);
        if pa.sub(pb).length() <= tol.linear * 4.0 {
            dropped_short_segments += 1;
            continue;
        }
        let ia = nodes.get_or_insert(
            NodeKey {
                obj: *oi,
                point: PointKey::Vertex(*ka),
            },
            pa,
            to2d(pa),
        );
        let ib = nodes.get_or_insert(
            NodeKey {
                obj: *oi,
                point: PointKey::Vertex(*kb),
            },
            pb,
            to2d(pb),
        );
        section_triangles += 1;
        segs.push(ia, ib, &edge.source, SegOrigin::CoplanarPatch);
    }

    // Grazing mesh edge crossed by surface geometry on both sides.
    for ((oi, ka, kb), edge) in graze_edges.iter() {
        let (Some(src_pos), Some(src_neg)) = (&edge.pos, &edge.neg) else {
            continue;
        };
        let obj = &scene.objects[*oi];
        let pa = plane.project(obj.positions[*ka as usize]);
        let pb = plane.project(obj.positions[*kb as usize]);
        if pa.sub(pb).length() <= tol.linear * 4.0 {
            dropped_short_segments += 1;
            continue;
        }
        let ia = nodes.get_or_insert(
            NodeKey {
                obj: *oi,
                point: PointKey::Vertex(*ka),
            },
            pa,
            to2d(pa),
        );
        let ib = nodes.get_or_insert(
            NodeKey {
                obj: *oi,
                point: PointKey::Vertex(*kb),
            },
            pb,
            to2d(pb),
        );
        if segs.push(ia, ib, src_pos, SegOrigin::GrazeEdge).is_some() {
            crossing_edge_emitted += 1;
            // Union the opposite-side face too.
            let key = (ia.min(ib), ia.max(ib));
            if let Some(&si) = segs.by_pair.get(&key) {
                let seg = &mut segs.segs[si];
                if !seg.sources.iter().any(|s| {
                    s.object_id == src_neg.object_id
                        && s.face_id == src_neg.face_id
                        && s.triangle_index == src_neg.triangle_index
                }) {
                    seg.sources.push(src_neg.clone());
                }
            }
        }
    }

    // --- tracing --------------------------------------------------------
    // Pair incident segments at every node before walking. A plane that passes
    // exactly through a mesh edge produces a degree-4 junction (the two
    // triangles on each side of the edge meet there); pairing the straightest
    // continuations once, up front, makes the walk deterministic and prevents
    // a closed ring from being split into two open chains at the junction.
    let mut adjacency: Vec<Vec<usize>> = vec![Vec::new(); nodes.nodes.len()];
    for (si, s) in segs.segs.iter().enumerate() {
        adjacency[s.a].push(si);
        adjacency[s.b].push(si);
    }
    for list in adjacency.iter_mut() {
        list.sort_unstable();
    }
    let pair_at: Vec<HashMap<usize, usize>> = adjacency
        .iter()
        .enumerate()
        .map(|(node, inc)| pair_incidents(node, inc, &segs.segs, &nodes.nodes))
        .collect();
    let mut used = vec![false; segs.segs.len()];
    let mut loops: Vec<LoopBuild> = Vec::new();
    let mut starts: Vec<usize> = adjacency
        .iter()
        .enumerate()
        .filter(|(_, a)| a.len() == 1)
        .map(|(i, _)| i)
        .collect();
    starts.sort_unstable();
    for start in starts {
        trace_paired(
            start,
            &adjacency,
            &pair_at,
            &segs.segs,
            &nodes.nodes,
            &mut used,
            &mut loops,
        );
    }
    for start in 0..nodes.nodes.len() {
        if adjacency[start].iter().any(|&si| !used[si]) {
            trace_paired(
                start,
                &adjacency,
                &pair_at,
                &segs.segs,
                &nodes.nodes,
                &mut used,
                &mut loops,
            );
        }
    }

    // --- rings, nesting, orientation ------------------------------------
    let mut rings = build_rings(loops, &nodes.nodes, &segs, tol, &mut diagnostics);
    assign_nesting(&mut rings, tol, &mut diagnostics);
    for r in rings.iter_mut() {
        if !r.closed {
            continue;
        }
        let area = signed_area(&r.pts2);
        let want_ccw = r.kind == "outer";
        if area != 0.0 && (area > 0.0) != want_ccw {
            r.pts2.reverse();
            r.pts3.reverse();
            r.node_ids.reverse();
            // Edge k joins node k to k+1; after reversing the node order the
            // edge sources must be rotated, not simply reversed, to stay
            // aligned. Rotating by one restores the same physical edges.
            if !r.edge_sources.is_empty() {
                r.edge_sources.rotate_right(1);
                r.edge_sources.reverse();
            }
        }
        r.signed_area = signed_area(&r.pts2);
    }

    for r in rings.iter().filter(|r| !r.closed) {
        diagnostics.push(Diagnostic {
            code: "openChain".into(),
            severity: "warning".into(),
            message: format!(
                "open chain {} in group '{}' is not closed ({} nodes, {} segments); no area or cap is fabricated. The endpoints are left as-is so the gap can be inspected.",
                r.id,
                r.group,
                r.node_ids.len(),
                r.edge_sources.len()
            ),
            object_id: r.object_ids.first().cloned(),
            contour_id: Some(r.id.clone()),
        });
    }

    // --- caps -----------------------------------------------------------
    let mut caps: Vec<CapOut> = Vec::new();
    for r in rings.iter().filter(|r| r.closed && r.kind == "outer") {
        let children: Vec<&Ring> = rings
            .iter()
            .filter(|h| {
                h.closed && h.kind == "hole" && h.parent_id.as_deref() == Some(r.id.as_str())
            })
            .collect();
        match triangulate_cap(r, &children, &plane, scene) {
            Ok(cap) => caps.push(cap),
            Err(reason) => diagnostics.push(Diagnostic {
                code: "capTriangulationFailed".into(),
                severity: "error".into(),
                message: format!("could not triangulate cap for {}: {}", r.id, reason),
                object_id: None,
                contour_id: Some(r.id.clone()),
            }),
        }
    }

    // --- section diagnostics -------------------------------------------
    let has_closed = rings.iter().any(|r| r.closed);
    let open_chains = rings.iter().filter(|r| !r.closed).count();
    if rings.is_empty() {
        diagnostics.push(diag(
            "emptySection",
            "info",
            "the plane does not intersect the model surface",
        ));
        if tangent_triangles > 0 {
            diagnostics.push(diag(
                "tangentContact",
                "info",
                "plane contacts the surface only tangentially (vertex, edge or coplanar face); tangential contact encloses no section area",
            ));
        }
    } else if !has_closed && open_chains > 0 {
        diagnostics.push(diag(
            "noClosedContours",
            "warning",
            "all traced chains are open; no area or cap is fabricated",
        ));
    }
    if !on_plane_face_refs.is_empty() {
        diagnostics.push(diag(
            "coplanarFaces",
            "info",
            format!(
                "{} triangle(s) lie in the plane; their patch boundary is included exactly once",
                on_plane_face_refs.len()
            ),
        ));
    }
    if crossing_edge_emitted > 0 {
        diagnostics.push(diag(
            "crossingEdges",
            "info",
            format!(
                "{} mesh edge(s) lie in the plane with surface on both sides and were emitted once",
                crossing_edge_emitted
            ),
        ));
    }
    if segs.merged > 0 {
        diagnostics.push(diag(
            "mergedCoincidentSegments",
            "info",
            format!(
                "{} coincident segment(s) from shared or duplicated faces were merged",
                segs.merged
            ),
        ));
    }

    // --- output ---------------------------------------------------------
    let contour_out = rings
        .iter()
        .map(|r| {
            let mut ds = r.diagnostics.clone();
            if !r.closed {
                ds.push("openChain; area intentionally not computed".into());
            }
            ContourOut {
                id: r.id.clone(),
                group: r.group.clone(),
                kind: r.kind.clone(),
                closed: r.closed,
                parent_id: r.parent_id.clone(),
                depth: r.depth,
                orientation: if r.closed {
                    Some(if r.signed_area >= 0.0 { "ccw" } else { "cw" }.to_string())
                } else {
                    None
                },
                points2d: r.pts2.clone(),
                points3d: r.pts3.clone(),
                signed_area: if r.closed { Some(r.signed_area) } else { None },
                area_square_meters: if r.closed {
                    Some(r.signed_area.abs() * scene.meters_per_unit * scene.meters_per_unit)
                } else {
                    None
                },
                object_ids: r.object_ids.clone(),
                face_refs: dedupe_refs(
                    r.sources
                        .iter()
                        .map(|s| FaceRef {
                            object_id: s.object_id.clone(),
                            face_id: s.face_id,
                            triangle_index: s.triangle_index,
                        })
                        .collect(),
                ),
                segments: if include_segments {
                    Some(ring_segments(r, &nodes.nodes))
                } else {
                    None
                },
                diagnostics: ds,
            }
        })
        .collect::<Vec<_>>();

    let mut result = SectionResult {
        schema: "archisection.result/1".into(),
        result_id: String::new(),
        scene_name: scene.name.clone(),
        unit: UnitOut {
            name: scene.unit_name.clone(),
            meters_per_unit: scene.meters_per_unit,
            area_unit: "m2".into(),
        },
        coordinate_system: "Right-handed world coordinates. +X right, +Y forward, +Z up. Lengths are in the scene model unit.".into(),
        bounds: BoundsOut {
            min: vec3_arr(scene.aabb.min),
            max: vec3_arr(scene.aabb.max),
            diagonal: scene.aabb.diagonal(),
        },
        plane: PlaneOut {
            normal: vec3_arr(n),
            offset: d,
            origin: vec3_arr(origin),
            basis_u: vec3_arr(u),
            basis_v: vec3_arr(v),
            basis_crosses_normal: u.cross(v).sub(n).length() <= 1e-9,
            definition: "Plane: dot(normal, p) = offset, with a unit normal. In-plane coordinates: points2d[i] = (points3d[i] - origin) . (basis_u, basis_v). basis_u x basis_v = normal. All lengths use the stated model unit.".into(),
        },
        tolerance: ToleranceOut {
            linear: tol.linear,
            angular: tol.angular,
            rules: vec![
                "A vertex is on the plane when |dot(n,p)-d| <= linear tolerance.".into(),
                "A triangle with vertices strictly on both sides is 'crossing'; its intersection points are keyed by (object, mesh vertex) or (object, unordered mesh edge), so neighbouring triangles share the exact same node.".into(),
                "A triangle whose three vertices are all on the plane is 'coplanar'. A coplanar mesh edge is emitted only when its adjacent coplanar triangles do not cancel (interior edges of a patch are never emitted; duplicated coincident faces emit one outline).".into(),
                "A triangle touching the plane at a vertex or edge without a sign change is 'grazing'. A grazing mesh edge is emitted only if triangles exist on both sides of it, so tangency produces no line.".into(),
                "Segments shorter than 4x linear tolerance are dropped and counted. Duplicate topological segments are merged and their face sources unioned.".into(),
                "Ring orientation is normalised by nesting: outer rings CCW and holes CW in the (u,v) basis.".into(),
                "Contours are connected by shared topological nodes only. At a non-manifold node the walk prefers a segment from the same group, then the straightest continuation. Disconnected components are never joined by nearest endpoints.".into(),
            ],
        },
        contours: contour_out,
        caps,
        diagnostics,
        stats: StatsOut {
            input_triangles: scene.objects.iter().map(|o| o.triangles.len()).sum(),
            section_triangles,
            segments: segs.segs.len(),
            nodes: nodes.nodes.len(),
            closed_contours: rings.iter().filter(|r| r.closed).count(),
            open_chains,
            holes: rings.iter().filter(|r| r.closed && r.kind == "hole").count(),
            caps: 0,
            coplanar_triangles: on_plane_face_refs.len(),
            on_plane_vertices,
            dropped_short_segments,
            merged_duplicate_segments: segs.merged,
            tangent_triangles,
            crossing_edge_emitted,
        },
        objects: scene
            .objects
            .iter()
            .map(|o| ObjectOut {
                id: o.id.clone(),
                name: o.name.clone(),
                group: o.group.clone(),
                triangles: o.triangles.len(),
                mirrored: o.mirrored,
            })
            .collect(),
        on_plane_face_refs: dedupe_refs(on_plane_face_refs),
    };
    result.stats.caps = result.caps.len();
    let canonical = serde_json::to_string(&(
        &result.scene_name,
        &result.unit,
        &result.plane,
        &result.tolerance.linear,
        &result.contours,
        &result.caps,
        &result.diagnostics,
    ))
    .unwrap_or_default();
    result.result_id = format!("sec-{:016x}", fnv1a64(canonical.as_bytes()));
    Ok(result)
}

// ---------------------------------------------------------------------------
// Geometry helpers
// ---------------------------------------------------------------------------

fn classify(sd: f64, tol: f64) -> i8 {
    if sd.abs() <= tol {
        0
    } else if sd > 0.0 {
        1
    } else {
        -1
    }
}

fn tri_pos(t: [u32; 3], v: u32) -> usize {
    if t[0] == v {
        0
    } else if t[1] == v {
        1
    } else {
        2
    }
}

fn canon_edge(a: u32, b: u32) -> (u32, u32) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

fn cross2(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn farthest_pair(cand: &[(PointKey, Vec3, [f64; 2])]) -> (usize, usize) {
    let mut best = (0, 1);
    let mut best_d = -1.0;
    for i in 0..cand.len() {
        for j in (i + 1)..cand.len() {
            let d = cand[i].1.sub(cand[j].1).length();
            if d > best_d {
                best_d = d;
                best = (i, j);
            }
        }
    }
    best
}

// ---------------------------------------------------------------------------
// Tracing
// ---------------------------------------------------------------------------

fn seg_groups(seg: &Segment) -> BTreeSet<&str> {
    seg.sources.iter().map(|s| s.group.as_str()).collect()
}

/// Pairs incident segments at one node. Each segment is paired with the
/// continuation a walk should take when it arrives through it. Degree-2 nodes
/// pair trivially; higher-degree (non-manifold) junctions prefer a segment
/// from the same group, then the straightest geometric continuation. The
/// pairing is symmetric, so a single decision governs both walk directions.
fn pair_incidents(
    node: usize,
    inc: &[usize],
    segments: &[Segment],
    node_store: &[Node],
) -> HashMap<usize, usize> {
    let mut pairs = HashMap::new();
    if inc.len() < 2 {
        return pairs;
    }
    let mut remaining: Vec<usize> = inc.to_vec();
    while remaining.len() >= 2 {
        let si = remaining[0];
        let in_group = seg_groups(&segments[si]);
        // Direction the walk is travelling when it reaches `node` through si.
        let prev = other_end(&segments[si], node);
        let din = node_store[node].pos3.sub(node_store[prev].pos3);
        let mut best: Option<(usize, f64)> = None;
        for &cand in remaining.iter().skip(1) {
            let next = other_end(&segments[cand], node);
            let d_out = node_store[next].pos3.sub(node_store[node].pos3);
            let same_group = seg_groups(&segments[cand])
                .iter()
                .any(|g| in_group.contains(g));
            let straight = din.normalize().dot(d_out.normalize());
            // Same-group continuations always outrank cross-group ones; within
            // a class the straightest line wins, ties break by segment id.
            let rank = if same_group { 10.0 } else { 0.0 } + straight;
            match best {
                Some((_, br)) if br >= rank => {}
                _ => best = Some((cand, rank)),
            }
        }
        let partner = best.expect("a continuation exists while >=2 remain").0;
        pairs.insert(si, partner);
        pairs.insert(partner, si);
        remaining.retain(|&x| x != si && x != partner);
    }
    pairs
}

/// Walks segment chains using the precomputed per-node pairings. Endpoints of
/// an open mesh start open chains; every other unused segment starts a closed
/// loop. The fixed pairing means the same junction is resolved identically no
/// matter which segment reaches it first.
fn trace_paired(
    start: usize,
    adjacency: &[Vec<usize>],
    pair_at: &[HashMap<usize, usize>],
    segments: &[Segment],
    node_store: &[Node],
    used: &mut [bool],
    loops: &mut Vec<LoopBuild>,
) {
    loop {
        let Some(&first) = adjacency[start].iter().find(|&&si| !used[si]) else {
            return;
        };
        let mut node_ids = vec![start];
        let mut edge_ids = vec![first];
        used[first] = true;
        let mut cur = other_end(&segments[first], start);
        let mut closed = false;
        let mut local_diags: Vec<String> = Vec::new();
        if adjacency[start].len() != 2 {
            local_diags.push("nonManifoldJunction; straightest paired continuation chosen".into());
        }
        let guard_limit = node_store.len() + 2;
        let mut guard = guard_limit;
        while guard > 0 {
            guard -= 1;
            if cur == start {
                closed = true;
                break;
            }
            node_ids.push(cur);
            // The node's pairing table names the continuation of the segment
            // we arrived on. No partner means an open mesh boundary.
            let incoming = *edge_ids.last().unwrap();
            let next_seg = match pair_at[cur].get(&incoming) {
                Some(&si) if !used[si] => si,
                _ => break,
            };
            if adjacency[cur].len() != 2 {
                local_diags.push("nonManifoldJunction; straightest paired continuation chosen".into());
            }
            used[next_seg] = true;
            edge_ids.push(next_seg);
            cur = other_end(&segments[next_seg], cur);
        }
        if guard == 0 {
            local_diags.push("traceSafetyLimitReached".into());
        }

        let mut sources: Vec<Source> = Vec::new();
        let mut groups: BTreeMap<String, usize> = BTreeMap::new();
        for &si in &edge_ids {
            for s in &segments[si].sources {
                if !sources.iter().any(|o| {
                    o.object_id == s.object_id
                        && o.face_id == s.face_id
                        && o.triangle_index == s.triangle_index
                }) {
                    sources.push(s.clone());
                }
                *groups.entry(s.group.clone()).or_insert(0) += 1;
            }
        }
        let group = groups
            .iter()
            .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(a.0)))
            .map(|(g, _)| g.clone())
            .unwrap_or_else(|| "default".into());
        if groups.len() > 1 {
            local_diags.push("contourSpansGroups".into());
        }
        loops.push(LoopBuild {
            group,
            closed: closed && node_ids.len() >= 3,
            node_ids,
            edge_ids,
            sources,
            diagnostics: local_diags,
        });
    }
}

fn other_end(s: &Segment, a: usize) -> usize {
    if s.a == a {
        s.b
    } else {
        s.a
    }
}

// ---------------------------------------------------------------------------
// Rings, nesting, caps
// ---------------------------------------------------------------------------

struct Ring {
    id: String,
    group: String,
    kind: String,
    closed: bool,
    parent_id: Option<String>,
    depth: usize,
    pts2: Vec<[f64; 2]>,
    pts3: Vec<[f64; 3]>,
    signed_area: f64,
    sources: Vec<Source>,
    object_ids: Vec<String>,
    node_ids: Vec<usize>,
    /// One source list per ring edge, aligned with node_ids (edge k runs from
    /// node k to node k+1, wrapping when closed).
    edge_sources: Vec<Vec<Source>>,
    diagnostics: Vec<String>,
}

fn build_rings(
    loops: Vec<LoopBuild>,
    nodes: &[Node],
    segs: &SegmentStore,
    tol: Tolerance,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<Ring> {
    let mut rings = Vec::new();
    for (i, l) in loops.into_iter().enumerate() {
        let mut ring = Ring {
            id: format!("C{}", i + 1),
            group: l.group,
            kind: if l.closed { "outer".into() } else { "open".into() },
            closed: l.closed,
            parent_id: None,
            depth: 0,
            pts2: l.node_ids.iter().map(|&n| nodes[n].pos2).collect(),
            pts3: l.node_ids.iter().map(|&n| vec3_arr(nodes[n].pos3)).collect(),
            signed_area: 0.0,
            sources: l.sources,
            object_ids: Vec::new(),
            node_ids: l.node_ids,
            edge_sources: l
                .edge_ids
                .iter()
                .map(|&si| segs.segs.get(si).map(|s| s.sources.clone()).unwrap_or_default())
                .collect(),
            diagnostics: l.diagnostics,
        };
        // Drop a repeated closing point (some traces include start twice).
        if ring.pts2.len() >= 2 {
            let f = ring.pts2[0];
            let last = *ring.pts2.last().unwrap();
            if (f[0] - last[0]).abs() <= tol.linear && (f[1] - last[1]).abs() <= tol.linear {
                ring.pts2.pop();
                ring.pts3.pop();
                ring.node_ids.pop();
                ring.edge_sources.pop();
            }
        }
        // Verify the raw trail before merging: at this point every consecutive
        // node pair must be backed by a traced segment. After collinear
        // merging a pair represents a straight chain, so the direct-pair
        // check no longer applies.
        let pair_count = if ring.closed {
            ring.node_ids.len()
        } else {
            ring.node_ids.len().saturating_sub(1)
        };
        let mut consistent = true;
        for k in 0..pair_count {
            let a = ring.node_ids[k];
            let b = ring.node_ids[(k + 1) % ring.node_ids.len()];
            if !segs.by_pair.contains_key(&(a.min(b), a.max(b))) {
                consistent = false;
            }
        }
        if !consistent {
            diagnostics.push(diag(
                "traceInconsistent",
                "error",
                format!("ring {} is not backed by a complete segment trail", ring.id),
            ));
        }
        // Remove collinear intermediate nodes created by the triangulation of a
        // flat face. Their presence would otherwise appear as extra contour
        // vertices and double-counted short edges.
        collapse_collinear(&mut ring, tol.linear);
        if ring.closed && (ring.pts2.len() < 3 || ring.edge_sources.len() != ring.pts2.len()) {
            ring.closed = false;
            ring.kind = "open".into();
            ring.diagnostics.push("degenerateRing".into());
        }
        if ring.closed {
            let area = signed_area(&ring.pts2).abs();
            if area <= tol.linear * tol.linear * 8.0 {
                diagnostics.push(diag(
                    "degenerateContour",
                    "warning",
                    format!("contour {} has negligible area and was dropped", ring.id),
                ));
                continue;
            }
        }
        ring.object_ids = ring
            .sources
            .iter()
            .map(|s| s.object_id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        ring.signed_area = signed_area(&ring.pts2);
        rings.push(ring);
    }
    rings
}

/// Merge consecutive edges that are collinear within tolerance. The merged
/// edge inherits the union of both source lists. This is what turns the six
/// triangle edges of a box wall into a single contour edge, and prevents
/// triangulation diagonals from polluting the contour.
fn collapse_collinear(ring: &mut Ring, tol: f64) {
    let closed = ring.closed;
    loop {
        let n = ring.pts2.len();
        if n < 3 || ring.edge_sources.len() != if closed { n } else { n - 1 } {
            return;
        }
        let mut found: Option<usize> = None;
        let first = if closed { 0 } else { 1 };
        let last = if closed { n } else { n - 1 };
        for i in first..last {
            let prev = if i == 0 { n - 1 } else { i - 1 };
            let next = if i + 1 == n { 0 } else { i + 1 };
            if is_straight(ring.pts2[prev], ring.pts2[i], ring.pts2[next], tol) {
                found = Some(i);
                break;
            }
        }
        let Some(i) = found else { return };
        let prev_edge = if i == 0 { ring.edge_sources.len() - 1 } else { i - 1 };
        let outgoing = i.min(ring.edge_sources.len() - 1);
        let mut merged = ring.edge_sources[prev_edge].clone();
        for s in &ring.edge_sources[outgoing] {
            if !merged.iter().any(|o| {
                o.object_id == s.object_id
                    && o.face_id == s.face_id
                    && o.triangle_index == s.triangle_index
            }) {
                merged.push(s.clone());
            }
        }
        ring.edge_sources[prev_edge] = merged;
        ring.edge_sources.remove(outgoing);
        ring.pts2.remove(i);
        ring.pts3.remove(i);
        ring.node_ids.remove(i);
        // Loop again: removing a node can expose new straight runs.
    }
}

/// True when a, b, c form a straight continuation (b lies between a and c).
fn is_straight(a: [f64; 2], b: [f64; 2], c: [f64; 2], tol: f64) -> bool {
    let d1 = [b[0] - a[0], b[1] - a[1]];
    let d2 = [c[0] - b[0], c[1] - b[1]];
    let l1 = (d1[0] * d1[0] + d1[1] * d1[1]).sqrt();
    let l2 = (d2[0] * d2[0] + d2[1] * d2[1]).sqrt();
    if l1 <= tol || l2 <= tol {
        return true;
    }
    let cross = d1[0] * d2[1] - d1[1] * d2[0];
    // Perpendicular distance of b from line a-c is |cross|/|c-a|.
    let ca = [c[0] - a[0], c[1] - a[1]];
    let lca = (ca[0] * ca[0] + ca[1] * ca[1]).sqrt();
    if lca <= tol {
        return false;
    }
    let distance = cross.abs() / lca;
    let forward = d1[0] * d2[0] + d1[1] * d2[1] > 0.0;
    distance <= tol && forward
}

fn ring_segments(r: &Ring, nodes: &[Node]) -> Vec<SegmentOut> {
    let n = r.node_ids.len();
    let pair_count = if r.closed { n } else { n.saturating_sub(1) };
    let mut out = Vec::with_capacity(pair_count);
    for k in 0..pair_count {
        let a = r.node_ids[k];
        let b = r.node_ids[(k + 1) % n];
        let sources = r.edge_sources.get(k).cloned().unwrap_or_default();
        out.push(SegmentOut {
            a: nodes[a].pos2,
            b: nodes[b].pos2,
            a3d: vec3_arr(nodes[a].pos3),
            b3d: vec3_arr(nodes[b].pos3),
            sources: dedupe_refs(
                sources
                    .iter()
                    .map(|s| FaceRef {
                        object_id: s.object_id.clone(),
                        face_id: s.face_id,
                        triangle_index: s.triangle_index,
                    })
                    .collect(),
            ),
        });
    }
    out
}

fn assign_nesting(rings: &mut [Ring], tol: Tolerance, diagnostics: &mut Vec<Diagnostic>) {
    let closed: Vec<usize> = rings
        .iter()
        .enumerate()
        .filter(|(_, r)| r.closed)
        .map(|(i, _)| i)
        .collect();
    let mut order: Vec<(usize, f64)> = closed
        .iter()
        .map(|&i| (i, signed_area(&rings[i].pts2).abs()))
        .collect();
    order.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });

    for idx in 0..order.len() {
        let child = order[idx].0;
        let child_area = order[idx].1;
        let mut parent: Option<usize> = None;
        let mut best_parent_area = f64::INFINITY;
        let mut boundary_touch = false;
        for &(cand, cand_area) in order[..idx].iter() {
            if cand == child || rings[cand].group != rings[child].group {
                continue;
            }
            if cand_area <= child_area + 8.0 * tol.linear * tol.linear {
                continue;
            }
            match containment(&rings[child].pts2, &rings[cand].pts2, tol.linear) {
                Containment::Inside => {
                    if cand_area < best_parent_area {
                        best_parent_area = cand_area;
                        parent = Some(cand);
                    }
                }
                Containment::OnBoundary => boundary_touch = true,
                Containment::Outside => {}
            }
        }
        if boundary_touch && parent.is_none() {
            diagnostics.push(diag(
                "contourTouchAmbiguous",
                "warning",
                format!(
                    "contour {} touches another contour within tolerance; it is not treated as its hole",
                    rings[child].id
                ),
            ));
        }
        rings[child].parent_id = parent.map(|p| rings[p].id.clone());
        rings[child].depth = parent.map(|p| rings[p].depth + 1).unwrap_or(0);
        rings[child].kind = if rings[child].depth % 2 == 0 {
            "outer".into()
        } else {
            "hole".into()
        };
    }
}

enum Containment {
    Inside,
    Outside,
    OnBoundary,
}

fn containment(child: &[[f64; 2]], parent: &[[f64; 2]], tol: f64) -> Containment {
    let mut any_inside = false;
    let mut any_boundary = false;
    for p in child {
        match point_in_polygon(*p, parent, tol) {
            Pip::Inside => any_inside = true,
            Pip::Boundary => any_boundary = true,
            Pip::Outside => return Containment::Outside,
        }
    }
    if any_inside {
        Containment::Inside
    } else if any_boundary {
        Containment::OnBoundary
    } else {
        Containment::Outside
    }
}

enum Pip {
    Inside,
    Outside,
    Boundary,
}

fn point_in_polygon(p: [f64; 2], poly: &[[f64; 2]], tol: f64) -> Pip {
    let n = poly.len();
    if n < 3 {
        return Pip::Outside;
    }
    for i in 0..n {
        let a = poly[i];
        let b = poly[(i + 1) % n];
        let ab = [b[0] - a[0], b[1] - a[1]];
        let ap = [p[0] - a[0], p[1] - a[1]];
        let len2 = ab[0] * ab[0] + ab[1] * ab[1];
        if len2 <= tol * tol {
            continue;
        }
        let t = ((ap[0] * ab[0] + ap[1] * ab[1]) / len2).clamp(0.0, 1.0);
        let q = [a[0] + ab[0] * t, a[1] + ab[1] * t];
        if (p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) <= tol * tol {
            return Pip::Boundary;
        }
    }
    let mut inside = false;
    for i in 0..n {
        let a = poly[i];
        let b = poly[(i + 1) % n];
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x_int = a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
            if x_int > p[0] {
                inside = !inside;
            }
        }
    }
    if inside {
        Pip::Inside
    } else {
        Pip::Outside
    }
}

fn signed_area(poly: &[[f64; 2]]) -> f64 {
    let n = poly.len();
    if n < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in 0..n {
        let a = poly[i];
        let b = poly[(i + 1) % n];
        sum += a[0] * b[1] - b[0] * a[1];
    }
    sum * 0.5
}

fn triangulate_cap(
    outer: &Ring,
    holes: &[&Ring],
    plane: &Plane,
    scene: &WorldScene,
) -> Result<CapOut, String> {
    if outer.pts2.len() < 3 {
        return Err("outer ring has fewer than 3 points".into());
    }
    let (u, v) = plane.basis();
    let mut flat: Vec<f64> = outer.pts2.iter().flat_map(|p| [p[0], p[1]]).collect();
    let mut hole_indices: Vec<usize> = Vec::new();
    for h in holes {
        if h.pts2.len() < 3 {
            continue;
        }
        hole_indices.push(flat.len() / 2);
        let mut ring = h.pts2.clone();
        if signed_area(&ring) > 0.0 {
            ring.reverse();
        }
        for p in &ring {
            flat.push(p[0]);
            flat.push(p[1]);
        }
    }
    let idx = earcutr::earcut(&flat, &hole_indices, 2)
        .map_err(|e| format!("triangulation error: {:?}", e))?;
    if idx.is_empty() {
        return Err("triangulation returned no triangles".into());
    }
    let mut triangles = Vec::with_capacity(idx.len() / 3);
    for chunk in idx.chunks(3) {
        if chunk.len() != 3 {
            continue;
        }
        let pts: Vec<Vec3> = chunk
            .iter()
            .map(|&i| {
                plane
                    .origin()
                    .add(u.scale(flat[i * 2]))
                    .add(v.scale(flat[i * 2 + 1]))
            })
            .collect();
        triangles.push([vec3_arr(pts[0]), vec3_arr(pts[1]), vec3_arr(pts[2])]);
    }
    let hole_area: f64 = holes.iter().map(|h| signed_area(&h.pts2).abs()).sum();
    let area = signed_area(&outer.pts2).abs() - hole_area;
    if area < 0.0 {
        return Err("hole area exceeds outer area".into());
    }
    let mut sources = outer.sources.clone();
    for h in holes {
        for s in &h.sources {
            if !sources.iter().any(|o| {
                o.object_id == s.object_id
                    && o.face_id == s.face_id
                    && o.triangle_index == s.triangle_index
            }) {
                sources.push(s.clone());
            }
        }
    }
    Ok(CapOut {
        id: format!("cap-{}", outer.id),
        group: outer.group.clone(),
        outer_contour_id: outer.id.clone(),
        hole_contour_ids: holes.iter().map(|h| h.id.clone()).collect(),
        triangles,
        area,
        area_square_meters: area * scene.meters_per_unit * scene.meters_per_unit,
        normal: vec3_arr(plane.n),
        object_ids: outer.object_ids.clone(),
        face_refs: dedupe_refs(
            sources
                .iter()
                .map(|s| FaceRef {
                    object_id: s.object_id.clone(),
                    face_id: s.face_id,
                    triangle_index: s.triangle_index,
                })
                .collect(),
        ),
    })
}

fn dedupe_refs(mut refs: Vec<FaceRef>) -> Vec<FaceRef> {
    let mut seen = HashSet::new();
    refs.retain(|r| seen.insert((r.object_id.clone(), r.face_id, r.triangle_index)));
    refs.sort_by(|a, b| {
        a.object_id
            .cmp(&b.object_id)
            .then(a.face_id.cmp(&b.face_id))
            .then(a.triangle_index.cmp(&b.triangle_index))
    });
    refs
}

fn diag(code: &str, severity: &str, msg: impl Into<String>) -> Diagnostic {
    Diagnostic {
        code: code.to_string(),
        severity: severity.to_string(),
        message: msg.into(),
        object_id: None,
        contour_id: None,
    }
}

fn vec3_arr(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
