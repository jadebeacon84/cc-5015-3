use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }
    pub fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
    pub fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
    pub fn scale(self, s: f64) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }
    pub fn dot(self, o: Vec3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }
    pub fn length(self) -> f64 {
        self.dot(self).sqrt()
    }
    pub fn normalize(self) -> Vec3 {
        let l = self.length();
        if l == 0.0 {
            Vec3::new(0.0, 0.0, 0.0)
        } else {
            self.scale(1.0 / l)
        }
    }
    pub fn lerp(self, o: Vec3, t: f64) -> Vec3 {
        self.add(o.sub(self).scale(t))
    }
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn empty() -> Self {
        Self {
            min: Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            max: Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        }
    }
    pub fn add(&mut self, p: Vec3) {
        self.min.x = self.min.x.min(p.x);
        self.min.y = self.min.y.min(p.y);
        self.min.z = self.min.z.min(p.z);
        self.max.x = self.max.x.max(p.x);
        self.max.y = self.max.y.max(p.y);
        self.max.z = self.max.z.max(p.z);
    }
    pub fn valid(&self) -> bool {
        self.min.x <= self.max.x
    }
    pub fn center(&self) -> Vec3 {
        Vec3::new(
            (self.min.x + self.max.x) * 0.5,
            (self.min.y + self.max.y) * 0.5,
            (self.min.z + self.max.z) * 0.5,
        )
    }
    pub fn diagonal(&self) -> f64 {
        if !self.valid() {
            return 0.0;
        }
        self.max.sub(self.min).length()
    }
}

/// Tolerance budget is derived from the model extents so results do not
/// depend on modelling units (mm vs m).  All geometric predicates in the
/// kernel use these values instead of hard-coded epsilons.
#[derive(Debug, Clone, Copy)]
pub struct Tolerance {
    pub linear: f64,
    pub angular: f64,
}

impl Tolerance {
    pub fn from_extents(diagonal: f64, unit_scale: f64) -> Self {
        let d = if diagonal.is_finite() && diagonal > 0.0 {
            diagonal
        } else {
            1.0
        };
        // 1e-7 of the model diagonal; clamped to a sane absolute band so that
        // both kilometre-scale and millimetre-scale models behave.
        let rel = d * 1e-7;
        let linear = rel.max(1e-9 * unit_scale).min(1e-2 * unit_scale);
        Self {
            linear,
            angular: 1e-9,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Plane {
    /// Unit normal in world space.
    pub n: Vec3,
    /// Signed distance from origin along `n`: `dot(n, p) = d` on the plane.
    pub d: f64,
}

impl Plane {
    pub fn signed_distance(&self, p: Vec3) -> f64 {
        self.n.dot(p) - self.d
    }
    pub fn project(&self, p: Vec3) -> Vec3 {
        p.sub(self.n.scale(self.signed_distance(p)))
    }
    /// Origin point of the plane (closest point to world origin).
    pub fn origin(&self) -> Vec3 {
        self.n.scale(self.d)
    }
    /// Right-handed in-plane basis (u, v) with u x v = n.
    pub fn basis(&self) -> (Vec3, Vec3) {
        let helper = if self.n.x.abs() < 0.9 {
            Vec3::new(1.0, 0.0, 0.0)
        } else {
            Vec3::new(0.0, 1.0, 0.0)
        };
        let u = helper.sub(self.n.scale(helper.dot(self.n))).normalize();
        let v = self.n.cross(u).normalize();
        (u, v)
    }
}
