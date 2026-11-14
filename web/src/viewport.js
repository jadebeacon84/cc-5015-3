import * as THREE from 'three';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';

/**
 * Builds the interactive model/section plane scene. The section itself is not
 * rendered by clipping here: the kernel computes real contours and caps, and
 * this module draws exactly what the kernel returned.
 */
export class Viewport {
  constructor(container, callbacks = {}) {
    this.container = container;
    this.callbacks = callbacks;
    this.objectGroups = new Map();
    this.planeNormal = new THREE.Vector3(0, 0, 1);
    this.planeOffset = 0;
    this.sectionVisible = true;
    this.modelOpacity = 0.32;
    this.dragMode = null;

    this.scene = new THREE.Scene();
    this.scene.background = null;

    const { clientWidth: w, clientHeight: h } = container;
    this.camera = new THREE.PerspectiveCamera(45, w / Math.max(h, 1), 0.01, 100000);
    this.camera.up.set(0, 0, 1); // world is Z-up
    this.camera.position.set(18, -22, 14);

    this.renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    this.renderer.setSize(w, h);
    this.renderer.localClippingEnabled = true;
    container.appendChild(this.renderer.domElement);

    this.controls = new OrbitControls(this.camera, this.renderer.domElement);
    this.controls.enableDamping = true;
    this.controls.dampingFactor = 0.08;
    this.controls.target.set(8, 3, 1.6);

    this.scene.add(new THREE.HemisphereLight(0xdfeaff, 0x2a2f3a, 1.15));
    const key = new THREE.DirectionalLight(0xffffff, 1.5);
    key.position.set(12, -18, 25);
    this.scene.add(key);
    const rim = new THREE.DirectionalLight(0x88aaff, 0.55);
    rim.position.set(-15, 12, 8);
    this.scene.add(rim);

    this.grid = new THREE.GridHelper(80, 80, 0x2a3644, 0x1a222d);
    this.grid.rotation.x = Math.PI / 2; // Three's grid is XZ; our world is Z-up.
    this.grid.position.set(8, 3, 0);
    this.scene.add(this.grid);    this.axes = new THREE.AxesHelper(3);
    this.axes.position.set(0, 0, 0);
    this.scene.add(this.axes);

    this.modelRoot = new THREE.Group();
    this.sectionRoot = new THREE.Group();
    this.planeRoot = new THREE.Group();
    this.selectionRoot = new THREE.Group();
    this.scene.add(this.modelRoot, this.sectionRoot, this.planeRoot, this.selectionRoot);

    this.planeMesh = this.makePlaneVisual();
    this.planeRoot.add(this.planeMesh);

    this.raycaster = new THREE.Raycaster();
    this.pointer = new THREE.Vector2();
    this.bindPointer();
    window.addEventListener('resize', () => this.resize());
    this.resize();

    this.renderer.setAnimationLoop(() => {
      this.controls.update();
      this.renderer.render(this.scene, this.camera);
    });
  }

  makePlaneVisual() {
    const geo = new THREE.PlaneGeometry(1, 1);
    const mat = new THREE.MeshBasicMaterial({
      color: 0x4db6ac,
      transparent: true,
      opacity: 0.07,
      side: THREE.DoubleSide,
      depthWrite: false,
    });
    const mesh = new THREE.Mesh(geo, mat);
    mesh.name = 'section-plane-visual';
    const edges = new THREE.LineSegments(
      new THREE.EdgesGeometry(geo),
      new THREE.LineBasicMaterial({ color: 0x67d8cd, transparent: true, opacity: 0.5 })
    );
    mesh.add(edges);
    // A grabbable handle at the plane centre: the transparent plane itself is
    // hard to hit when viewed edge-on, so dragging uses this sphere.
    const handle = new THREE.Mesh(
      new THREE.SphereGeometry(1, 16, 12),
      new THREE.MeshBasicMaterial({ color: 0x67d8cd, transparent: true, opacity: 0.9, depthTest: false })
    );
    handle.name = 'plane-handle';
    handle.renderOrder = 8;
    mesh.add(handle);
    mesh.userData.handle = handle;
    return mesh;
  }

  resize() {
    const { clientWidth: w, clientHeight: h } = this.container;
    if (w === 0 || h === 0) return;
    this.camera.aspect = w / h;
    this.camera.updateProjectionMatrix();
    this.renderer.setSize(w, h);
  }

  /** Loads a scene document (only for display; the kernel owns the truth). */
  setModel(doc) {
    this.clearModel();
    const root = new THREE.Group();
    const matColors = [0x6f8fb5, 0x7fa88f, 0x9a8fb8, 0xb59a7f, 0x7f9db5, 0xa88f8f];
    const box = new THREE.Box3();

    doc.objects.forEach((obj, i) => {
      const mesh = obj.mesh;
      if (!mesh || !mesh.positions || mesh.positions.length === 0) return;
      const geometry = new THREE.BufferGeometry();
      const pos = new Float32Array(mesh.positions);
      const index = mesh.indices.length > 65535 ? new Uint32Array(mesh.indices) : new Uint16Array(mesh.indices);
      geometry.setAttribute('position', new THREE.BufferAttribute(pos, 3));
      geometry.setIndex(new THREE.BufferAttribute(index, 1));

      const matrix = new THREE.Matrix4();
      if (obj.matrix && obj.matrix.length === 16) {
        matrix.fromArray(obj.matrix); // column-major, as Three expects
      }
      geometry.applyMatrix4(matrix);
      geometry.computeVertexNormals();
      geometry.computeBoundingBox();
      box.expandByObject(new THREE.Mesh(geometry));

      const color = obj.group
        ? new THREE.Color().setHSL((hashString(obj.group) % 360) / 360, 0.32, 0.58)
        : matColors[i % matColors.length];

      const solid = new THREE.Mesh(
        geometry,
        new THREE.MeshStandardMaterial({
          color,
          metalness: 0.05,
          roughness: 0.85,
          transparent: true,
          opacity: this.modelOpacity,
          side: THREE.DoubleSide,
          depthWrite: false,
        })
      );
      solid.userData = { kind: 'model', objectId: obj.id, faceIds: mesh.faceIds || null, group: obj.group };
      const wire = new THREE.LineSegments(
        new THREE.WireframeGeometry(geometry),
        new THREE.LineBasicMaterial({ color: 0x8fa6c4, transparent: true, opacity: 0.16 })
      );
      wire.userData = { kind: 'model-wire' };
      root.add(solid, wire);
      this.objectGroups.set(obj.id, { solid, wire, def: obj, color });
    });

    this.modelRoot.add(root);
    this.modelRoot.userData.root = root;
    this.frameBox(box);
  }

  clearModel() {
    this.objectGroups.clear();
    this.disposeChildren(this.modelRoot);
  }

  frameBox(box) {
    if (!box || box.isEmpty()) return;
    const centre = box.getCenter(new THREE.Vector3());
    const size = box.getSize(new THREE.Vector3());
    const radius = Math.max(size.length() * 0.5, 0.5);
    this.controls.target.copy(centre);
    const dir = new THREE.Vector3(0.9, -1.15, 0.7).normalize();
    this.camera.position.copy(centre).addScaledVector(dir, radius * 2.1);
    this.camera.near = Math.max(radius / 5000, 0.001);
    this.camera.far = radius * 500;
    this.camera.updateProjectionMatrix();
    this.controls.update();
    this.bounds = { box, centre, size, radius };
    this.updatePlaneVisual();
  }

  setPlane(normal, offset, extentHint) {
    this.planeNormal.set(normal[0], normal[1], normal[2]).normalize();
    this.planeOffset = offset;
    if (extentHint) {
      this.planeExtent = extentHint;
      this.planeOrigin = new THREE.Vector3(...extentHint.origin);
    }
    this.updatePlaneVisual();
  }

  /**
   * `offset` is the absolute plane constant d in dot(normal, p) = d, matching
   * the kernel exactly. The visual quad is centred on the projection of the
   * model centre onto that plane so it always covers the model.
   */
  updatePlaneVisual() {
    if (!this.bounds) return;
    const { centre, radius } = this.bounds;
    const n = this.planeNormal;
    const centreDist = n.dot(centre);
    const origin = centre.clone().addScaledVector(n, this.planeOffset - centreDist);
    // Extent of the model projected along the normal. Drag-to-move maps a
    // full viewport-height drag onto exactly this distance, so the plane
    // never flies off the model in a single gesture.
    const ext = this.bounds.box.max.clone().sub(this.bounds.box.min);
    this.planeExtent = Math.max(
      Math.abs(ext.x * n.x) + Math.abs(ext.y * n.y) + Math.abs(ext.z * n.z),
      1e-6
    );
    const q = new THREE.Quaternion().setFromUnitVectors(new THREE.Vector3(0, 0, 1), n);
    const size = radius * 3.2;
    this.planeMesh.position.copy(origin);
    this.planeMesh.quaternion.copy(q);
    this.planeMesh.scale.set(size, size, 1);
    this.planeMesh.userData.handle?.scale.setScalar(
      Math.max((radius * 0.045) / size, 0.001)
    );
    this.planeMesh.visible = this.planeVisible !== false && this.sectionVisible;
    this.planeOrigin = origin;
  }

  setSectionVisible(v) {
    this.sectionVisible = v;
    this.sectionRoot.visible = v;
  }

  setPlaneVisible(v) {
    this.planeVisible = v;
    this.planeMesh.visible = v && this.sectionVisible;
  }

  setModelOpacity(v) {
    this.modelOpacity = v;
    for (const { solid } of this.objectGroups.values()) {
      solid.material.opacity = v;
      solid.material.needsUpdate = true;
    }
  }

  /** Replaces the kernel-returned section rendering. */
  setSection(result) {
    this.disposeChildren(this.sectionRoot);
    if (!result) return;
    for (const contour of result.contours) {
      if (!contour.closed) this.addOpenChain(contour);
      else this.addContour(contour);
    }
    for (const cap of result.caps) this.addCap(cap);
  }

  addContour(contour) {
    const pts = contour.points3d.map((p) => new THREE.Vector3(p[0], p[1], p[2]));
    if (contour.closed && pts.length) pts.push(pts[0].clone());
    const geo = new THREE.BufferGeometry().setFromPoints(pts);
    const isHole = contour.kind === 'hole';
    const line = new THREE.Line(
      geo,
      new THREE.LineBasicMaterial({
        color: isHole ? 0xffb74d : 0x4dd0c4,
        linewidth: 2,
        depthTest: false,
        transparent: true,
        opacity: 0.98,
      })
    );
    line.renderOrder = 6;
    line.userData = { kind: 'contour', contourId: contour.id };
    this.sectionRoot.add(line);
  }

  addOpenChain(contour) {
    const pts = contour.points3d.map((p) => new THREE.Vector3(p[0], p[1], p[2]));
    const geo = new THREE.BufferGeometry().setFromPoints(pts);
    const line = new THREE.Line(
      geo,
      new THREE.LineDashedMaterial({
        color: 0xef5350,
        dashSize: 0.18,
        gapSize: 0.12,
        depthTest: false,
        transparent: true,
        opacity: 0.95,
      })
    );
    line.computeLineDistances();
    line.renderOrder = 6;
    line.userData = { kind: 'contour', contourId: contour.id };
    this.sectionRoot.add(line);
    // Endpoint markers make the diagnostic gap explicit.
    for (const end of [0, pts.length - 1]) {
      const dot = new THREE.Mesh(
        new THREE.SphereGeometry(Math.max(this.bounds.radius * 0.012, 0.01), 8, 8),
        new THREE.MeshBasicMaterial({ color: 0xef5350, depthTest: false })
      );
      dot.position.copy(pts[end]);
      dot.renderOrder = 7;
      dot.userData = { kind: 'open-end', contourId: contour.id };
      this.sectionRoot.add(dot);
    }
  }

  addCap(cap) {
    const verts = [];
    for (const tri of cap.triangles) {
      for (const p of tri) verts.push(p[0], p[1], p[2]);
    }
    const geo = new THREE.BufferGeometry();
    geo.setAttribute('position', new THREE.Float32BufferAttribute(verts, 3));
    geo.computeVertexNormals();
    // Caps are drawn double sided with a slight offset toward the viewer so
    // they read as filled surfaces without hiding the contour lines.
    const mat = new THREE.MeshBasicMaterial({
      color: 0x2f7f78,
      transparent: true,
      opacity: 0.5,
      side: THREE.DoubleSide,
      depthWrite: false,
      polygonOffset: true,
      polygonOffsetFactor: 1,
      polygonOffsetUnits: 1,
    });
    const mesh = new THREE.Mesh(geo, mat);
    mesh.renderOrder = 4;
    mesh.userData = { kind: 'cap', capId: cap.id };
    this.sectionRoot.add(mesh);
  }

  selectContour(contourId) {
    this.clearSelection();
    if (!contourId) return;
    this.sectionRoot.traverse((o) => {
      if (o.userData?.contourId === contourId) {
        o.material?.color?.set(0xffffff);
        if (o.material) o.material.opacity = 1;
      }
    });
  }

  clearSelection() {
    this.selectionRoot.clear();
  }

  /** Highlights the original faces that produced a contour or segment. */
  highlightFaces(refs) {
    this.clearSelection();
    if (!refs || refs.length === 0) return;
    for (const ref of refs) {
      const entry = this.objectGroups.get(ref.objectId);
      if (!entry) continue;
      const { solid, def } = entry;
      const geo = solid.geometry;
      const index = geo.getIndex();
      const pos = geo.getAttribute('position');
      if (!index) continue;
      const faceIds = def.mesh.faceIds;
      const triCount = index.count / 3;
      const verts = [];
      for (let t = 0; t < triCount; t++) {
        const fid = faceIds ? faceIds[t] : t;
        if (fid !== ref.faceId) continue;
        for (let k = 0; k < 3; k++) {
          const vi = index.getX(t * 3 + k);
          verts.push(pos.getX(vi), pos.getY(vi), pos.getZ(vi));
        }
      }
      if (verts.length === 0) continue;
      const g = new THREE.BufferGeometry();
      g.setAttribute('position', new THREE.Float32BufferAttribute(verts, 3));
      const m = new THREE.MeshBasicMaterial({
        color: 0xffd54f,
        transparent: true,
        opacity: 0.55,
        side: THREE.DoubleSide,
        depthTest: false,
      });
      const mesh = new THREE.Mesh(g, m);
      mesh.renderOrder = 5;
      this.selectionRoot.add(mesh);
    }
  }

  focusContour(contour) {
    if (!contour || contour.points3d.length === 0) return;
    const box = new THREE.Box3();
    for (const p of contour.points3d) box.expandByPoint(new THREE.Vector3(p[0], p[1], p[2]));
    const centre = box.getCenter(new THREE.Vector3());
    const size = box.getSize(new THREE.Vector3()).length();
    const dir = this.camera.position.clone().sub(this.controls.target).normalize();
    this.controls.target.copy(centre);
    this.camera.position.copy(centre).addScaledVector(dir, Math.max(size * 1.6, 1.5));
    this.controls.update();
  }

  // ------------------------------------------------------------ interaction
  bindPointer() {
    const el = this.renderer.domElement;
    let down = null;

    el.addEventListener('pointerdown', (e) => {
      if (e.button !== 0) return;
      const hit = this.pickPlane(e);
      if (hit) {
        this.dragMode = 'plane';
        down = { x: e.clientX, y: e.clientY, startOffset: this.planeOffset };
        this.controls.enabled = false;
        this.callbacks.onPlaneDragStart?.();
        el.setPointerCapture(e.pointerId);
        el.style.cursor = 'grabbing';
      } else {
        const contour = this.pickContour(e);
        if (contour) this.callbacks.onContourPick?.(contour);
      }
    });

    el.addEventListener('pointermove', (e) => {
      if (this.dragMode !== 'plane' || !down) return;
      // Project pointer motion onto the plane normal axis. A full-height drag
      // moves the plane across the model's full projected extent.
      const dy = e.clientY - down.y;
      const extent = this.planeExtent || this.bounds?.radius * 2 || 1;
      const delta = (-dy / Math.max(el.clientHeight, 1)) * extent;
      const next = down.startOffset + delta;
      this.callbacks.onPlaneDrag?.(next);
    });
    const end = (e) => {
      if (this.dragMode !== 'plane') return;
      this.dragMode = null;
      down = null;
      this.controls.enabled = true;
      el.style.cursor = 'grab';
      this.callbacks.onPlaneDragEnd?.();
      try { el.releasePointerCapture(e.pointerId); } catch {}
    };
    el.addEventListener('pointerup', end);
    el.addEventListener('pointercancel', end);
    el.style.cursor = 'grab';
  }

  pickPlane(event) {
    if (!this.planeMesh.visible) return false;
    const rect = this.renderer.domElement.getBoundingClientRect();
    this.pointer.set(
      ((event.clientX - rect.left) / rect.width) * 2 - 1,
      -((event.clientY - rect.top) / rect.height) * 2 + 1
    );
    this.raycaster.setFromCamera(this.pointer, this.camera);
    // Test the visual plane and its handle explicitly so the plane can be
    // grabbed even when the pointer is over the translucent quad.
    const targets = [this.planeMesh];
    if (this.planeMesh.userData.handle) targets.push(this.planeMesh.userData.handle);
    const hits = this.raycaster.intersectObjects(targets, true);
    return hits.length > 0;
  }

  pickContour(event) {
    const rect = this.renderer.domElement.getBoundingClientRect();
    this.pointer.set(
      ((event.clientX - rect.left) / rect.width) * 2 - 1,
      -((event.clientY - rect.top) / rect.height) * 2 + 1
    );
    this.raycaster.setFromCamera(this.pointer, this.camera);
    this.raycaster.params.Line.threshold = Math.max(this.bounds?.radius || 1, 1) * 0.01;
    const hits = this.raycaster.intersectObjects(this.sectionRoot.children, false);
    const hit = hits.find((h) => h.object.userData?.contourId);
    return hit?.object.userData.contourId || null;
  }

  disposeChildren(root) {
    const stack = [...root.children];
    while (stack.length) {
      const o = stack.pop();
      if (o.children?.length) stack.push(...o.children);
      o.geometry?.dispose?.();
      if (Array.isArray(o.material)) o.material.forEach((m) => m.dispose?.());
      else o.material?.dispose?.();
    }
    root.clear();
  }
}

function hashString(s) {
  let h = 0;
  for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) | 0;
  return Math.abs(h);
}
