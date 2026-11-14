import * as api from './api.js';
import { Viewport } from './viewport.js';
import { exportCsv, exportDxf, exportJson } from './export.js';
import './style.css';

const app = document.getElementById('app');
app.innerHTML = `
  <header class="topbar">
    <h1>模型剖切视窗</h1>
    <span class="sub" id="scene-name">未载入模型</span>
    <span class="spacer"></span>
    <span class="badge" id="kernel-status">检测内核…</span>
    <span class="badge" id="result-status">尚无结果</span>
    <span class="badge" id="freeze-status">未冻结</span>
  </header>

  <section class="viewport" id="viewport">
    <div class="viewport-overlay">
      <div class="top">
        <div class="hint">
          <b>左键拖拽青色平面</b>移动剖切位置；空白处拖拽旋转视角，滚轮缩放。<br>
          平面法向与偏移交给 Rust 内核计算真实交线，不是渲染裁切。
        </div>
      </div>
      <div class="bottom">
        <div class="hint" id="plane-hint">平面尚未计算</div>
      </div>
    </div>
  </section>

  <aside class="side">
    <div class="panel">
      <h2>模型</h2>
      <div class="readout" id="model-info">加载中…</div>
      <div style="height:8px"></div>
      <div class="field" style="margin-bottom:8px">
        <label>内置示例</label>
        <select id="fixture-select">
          <option value="demo-building.json">演示建筑（内腔 / 薄墙 / 旋转塔楼）</option>
          <option value="open-shell.json">开放壳体（缺少一面墙）</option>
        </select>
      </div>
      <div class="btn-row">
        <button class="btn" id="btn-demo">载入示例</button>
        <button class="btn ghost" id="btn-file">打开本地 JSON</button>
      </div>
      <input type="file" id="file-input" accept=".json,application/json" hidden>
    </div>

    <div class="panel">
      <h2>剖切平面 <span style="text-transform:none;color:#8b98aa;font-weight:400">n · p = d</span></h2>
      <div class="axis-row" id="axis-row">
        <button class="seg active" data-axis="z">Z 轴</button>
        <button class="seg" data-axis="y">Y 轴</button>
        <button class="seg" data-axis="x">X 轴</button>
        <button class="seg" data-axis="custom">自定义</button>
      </div>
      <div class="field">
        <label>偏移 d <span id="offset-value">0.000</span></label>
        <input type="range" id="offset-slider" min="-1" max="1" step="0.001" value="0">
      </div>
      <div class="row2">
        <div class="field">
          <label>d（模型单位）</label>
          <input type="number" id="offset-number" step="0.01" value="0">
        </div>
        <div class="field">
          <label>法向长度</label>
          <input type="text" id="normal-readout" readonly value="0, 0, 1">
        </div>
      </div>
      <div id="custom-normal" class="row2" style="display:none">
        <div class="field">
          <label>nX</label><input type="number" id="nx" step="0.1" value="0">
        </div>
        <div class="field">
          <label>nY</label><input type="number" id="ny" step="0.1" value="0">
        </div>
        <div class="field">
          <label>nZ</label><input type="number" id="nz" step="0.1" value="1">
        </div>
        <div class="field">
          <label>&nbsp;</label><button class="btn" id="btn-apply-normal">应用法向</button>
        </div>
      </div>
      <div class="btn-row">
        <button class="btn primary" id="btn-section">计算剖面</button>
        <button class="btn" id="btn-fit">复位视角</button>
      </div>
    </div>

    <div class="panel">
      <h2>显示</h2>
      <div class="checks">
        <label class="check"><input type="checkbox" id="chk-section" checked>显示剖面轮廓与封口</label>
        <label class="check"><input type="checkbox" id="chk-plane" checked>显示剖切平面</label>
        <label class="check"><input type="checkbox" id="chk-sources">选中轮廓时高亮原面</label>
        <label class="check"><input type="checkbox" id="chk-live" checked>拖拽平面时实时计算</label>
      </div>
      <div class="field" style="margin-top:9px">
        <label>模型透明度 <span id="opacity-value">0.32</span></label>
        <input type="range" id="opacity-slider" min="0.05" max="1" step="0.01" value="0.32">
      </div>
    </div>

    <div class="panel">
      <h2>结果冻结</h2>
      <div class="readout" id="freeze-info">移动平面后需要点击“冻结结果”才能锁定当前剖面用于导出与核对。</div>
      <div style="height:8px"></div>
      <div class="btn-row">
        <button class="btn primary" id="btn-freeze">冻结结果</button>
        <button class="btn" id="btn-thaw">解除冻结</button>
      </div>
    </div>

    <div class="panel">
      <h2>导出（含单位与平面定义）</h2>
      <div class="btn-row">
        <button class="btn" id="btn-export-json" disabled>JSON</button>
        <button class="btn" id="btn-export-dxf" disabled>DXF</button>
        <button class="btn" id="btn-export-csv" disabled>CSV</button>
      </div>
      <div class="freeze-note" id="export-note">冻结后可导出。</div>
    </div>
  </aside>

  <section class="results">
    <div class="results-col">
      <div class="results-head">轮廓 <span class="count" id="contour-count">0</span></div>
      <ul class="contour-list" id="contour-list"></ul>
    </div>
    <div class="results-col">
      <div class="results-head">诊断 <span class="count" id="diag-count">0</span></div>
      <div class="diag-list" id="diag-list"></div>
    </div>
    <div class="results-col">
      <div class="results-head">选中详情</div>
      <div class="detail" id="detail"><div class="empty">选择一个轮廓查看交点、面积与来源面。</div></div>
    </div>
  </section>
  <div class="toast" id="toast"></div>
`;

const el = (id) => document.getElementById(id);

const state = {
    scene: null,
    sceneId: null,
    sceneName: '',
    unit: { name: 'unit', metersPerUnit: 1 },
    bounds: null,
    normal: [0, 0, 1],
    offset: 0,
    sliderRange: { min: 0, max: 1 },
    result: null,
    frozen: null,
    selectedId: null,
    live: true,
    abort: null,
    pending: false,
    lastRequestAt: 0,
    requestCounter: 0,
};

const viewport = new Viewport(el('viewport'), {
  onPlaneDragStart: () => {
    // Freezing records the plane at that moment; a drag invalidates the
    // frozen snapshot and the export buttons are disabled until re-freeze.
    viewport.controls.enabled = false;
  },
  onPlaneDrag: (nextOffset) => {
    const clamped = clampOffset(nextOffset);
    state.offset = clamped;
    syncOffsetControls(clamped);
    viewport.setPlane(state.normal, clamped);
    checkFrozenStillMatches();
    if (state.live) scheduleSection();
    else markStale();
  },
  onPlaneDragEnd: () => {
    viewport.controls.enabled = true;
    if (state.live) scheduleSection(0);
  },
  onContourPick: (id) => {
    if (!id) return;
    selectContour(id);
  },
});

boot();

async function boot() {
  setStatus('kernel-status', '检测内核…', 'busy');
  try {
    await api.health();
    setStatus('kernel-status', '内核就绪', 'ok');
  } catch (err) {
    setStatus('kernel-status', '内核不可用', 'err');
    showToast(`无法连接 Rust 内核：${err.message}。请确认服务已启动。`);
    return;
  }
  try {
    await loadFixture('demo-building.json');
  } catch (err) {
    setStatus('kernel-status', '示例加载失败', 'err');
    showToast(err.message);
  }
}

async function loadFixture(name) {
  beginSceneLoad();
  const doc = await api.loadFixture(name);
  await adoptScene(doc, `fixtures/${name}`);
}

/**
 * Invalidates the current result the moment a new model load starts. This runs
 * synchronously inside the click handler so a stale "已计算" badge can never
 * be observed while the new scene is still in flight.
 */
function beginSceneLoad() {
  state.result = null;
  state.frozen = null;
  state.selectedId = null;
  viewport.clearSelection();
  setStatus('result-status', '载入中…', 'busy');
  setStatus('freeze-status', '未冻结', '');
  setContours();
  setDiagnostics();
  setDetail(null);
  updateExportButtons();
}

async function adoptScene(doc, label) {
  beginSceneLoad();
  const loaded = await api.loadScene(doc);
  state.scene = doc;
  state.sceneId = loaded.sceneId;
  state.sceneName = loaded.name;
  state.unit = loaded.unit;
  state.bounds = loaded.bounds;
  el('scene-name').textContent = `${loaded.name} · ${loaded.unit.name}`;
  el('model-info').textContent = describeScene(loaded);
  viewport.setModel(doc);
  viewport.setSection(null);
  el('freeze-info').textContent = '已载入新模型，尚未冻结结果。';
  // Start at a height that visibly cuts the model.
  state.normal = [0, 0, 1];
  syncNormalButtons();
  configureOffsetRange();
  const start = suggestedOffset();
  state.offset = start;
  syncOffsetControls(start);
  viewport.setPlane(state.normal, start);
  await computeSection();
  showToast(`已载入 ${label}`);
}

function describeScene(loaded) {
  const lines = [];
  lines.push(`scene  ${loaded.sceneId}`);
  lines.push(`对象  ${loaded.objects.length}`);
  for (const o of loaded.objects) {
    lines.push(`  ${o.id}  ${o.group}  tri=${o.triangles}${o.mirrored ? ' (mirrored)' : ''}`);
  }
  lines.push(`单位  ${loaded.unit.name}  (1 unit = ${loaded.unit.metersPerUnit} m)`);
  const b = loaded.bounds;
  lines.push(
    `范围  X ${fmt(b.min[0])}..${fmt(b.max[0])}  Y ${fmt(b.min[1])}..${fmt(b.max[1])}  Z ${fmt(b.min[2])}..${fmt(b.max[2])}`
  );
  for (const [w, n] of Object.entries(loaded.bakeWarnings || {})) {
    lines.push(`烘焙提示 ${w} × ${n}`);
  }
  return lines.join('\n');
}

function suggestedOffset() {
  if (!state.bounds) return 0;
  // Around a third of the way up the bounding box along the current normal.
  const n = state.normal;
  const corners = bboxCorners(state.bounds);
  let lo = Infinity;
  let hi = -Infinity;
  for (const p of corners) {
    const d = p[0] * n[0] + p[1] * n[1] + p[2] * n[2];
    lo = Math.min(lo, d);
    hi = Math.max(hi, d);
  }
  return lo + (hi - lo) * 0.38;
}

function configureOffsetRange() {
  if (!state.bounds) return;
  const n = state.normal;
  const corners = bboxCorners(state.bounds);
  let lo = Infinity;
  let hi = -Infinity;
  for (const p of corners) {
    const d = p[0] * n[0] + p[1] * n[1] + p[2] * n[2];
    lo = Math.min(lo, d);
    hi = Math.max(hi, d);
  }
  const margin = (hi - lo) * 0.15 + 1e-6;
  state.sliderRange = { min: lo - margin, max: hi + margin };
  const slider = el('offset-slider');
  slider.min = fmt(lo - margin);
  slider.max = fmt(hi + margin);
}

function bboxCorners(bounds) {
  const { min, max } = bounds;
  const out = [];
  for (const x of [min[0], max[0]])
    for (const y of [min[1], max[1]])
      for (const z of [min[2], max[2]]) out.push([x, y, z]);
  return out;
}

function clampOffset(v) {
  const { min, max } = state.sliderRange;
  return Math.min(max, Math.max(min, v));
}

function syncOffsetControls(v) {
  el('offset-slider').value = String(v);
  el('offset-number').value = fmtInput(v);
  el('offset-value').textContent = fmt(v);
  el('plane-hint').textContent = `平面 ${fmtNormal(state.normal)} · d = ${fmt(v)}`;
}

function syncNormalButtons() {
  const [x, y, z] = state.normal;
  let axis = 'custom';
  if (Math.abs(x) < 1e-9 && Math.abs(y) < 1e-9 && Math.abs(z) > 0) axis = 'z';
  else if (Math.abs(x) < 1e-9 && Math.abs(z) < 1e-9 && Math.abs(y) > 0) axis = 'y';
  else if (Math.abs(y) < 1e-9 && Math.abs(z) < 1e-9 && Math.abs(x) > 0) axis = 'x';
  document.querySelectorAll('.seg').forEach((b) => b.classList.toggle('active', b.dataset.axis === axis));
  el('custom-normal').style.display = axis === 'custom' ? '' : 'none';
  el('normal-readout').value = fmtNormal(state.normal);
  el('nx').value = fmtInput(state.normal[0]);
  el('ny').value = fmtInput(state.normal[1]);
  el('nz').value = fmtInput(state.normal[2]);
}

function fmtNormal(n) {
  return `(${n.map((v) => fmt(v)).join(', ')})`;
}

let rafPending = false;
let rafTimer = null;
function scheduleSection(delay = 60) {
  if (rafPending) return;
  rafPending = true;
  clearTimeout(rafTimer);
  rafTimer = setTimeout(async () => {
    rafPending = false;
    await computeSection();
  }, delay);
}

async function computeSection() {
  if (!state.sceneId) return;
  if (state.abort) state.abort.abort();
  state.abort = new AbortController();
  const requestNumber = ++state.requestCounter;
  setStatus('result-status', '计算中…', 'busy');
  try {
    const { result } = await api.section(
      {
        sceneId: state.sceneId,
        normal: state.normal,
        offset: state.offset,
        includeSegments: true,
      },
      state.abort.signal
    );
    if (requestNumber !== state.requestCounter) return; // superseded
    applyResult(result);
    setStatus('result-status', `已计算 ${result.contours.length} 条轮廓`, 'ok');
  } catch (err) {
    if (err.name === 'AbortError') return;
    if (requestNumber !== state.requestCounter) return;
    setStatus('result-status', '计算失败', 'err');
    showToast(`剖切失败：${err.message}`);
    // Keep the last good result visible but flag it as stale.
    markStale('计算失败，当前显示为上一次成功结果。');
  }
}

function applyResult(result) {
  state.result = result;
  state.selectedId = null;
  viewport.setSection(result);
  viewport.clearSelection();
  renderContours(result);
  renderDiagnostics(result);
  setDetail(null);
  updateExportButtons();
  if (!state.frozen) {
    el('freeze-info').textContent = `当前动态结果 ${result.resultId}；如需导出或核对，请冻结。`;
    setStatus('freeze-status', '未冻结', '');
  }
}

function markStale(message) {
  if (state.frozen) {
    setStatus('freeze-status', '冻结已过期', 'err');
    el('freeze-info').textContent = '平面已移动，冻结结果不再是当前平面的剖面。重新冻结以更新。';
  } else if (message) {
    showToast(message);
  }
  updateExportButtons();
}

function renderContours(result) {
  const list = el('contour-list');
  list.innerHTML = '';
  el('contour-count').textContent = String(result.contours.length);
  if (result.contours.length === 0) {
    list.innerHTML = '<li class="empty">此平面与模型没有产生相交轮廓。</li>';
    return;
  }
  result.contours.forEach((c, i) => {
    const li = document.createElement('li');
    li.className = 'contour-item';
    li.dataset.contourId = c.id;
    const color = c.kind === 'hole' ? '#ffb74d' : c.kind === 'open' ? '#ef5350' : '#4dd0c4';
    const area = c.areaSquareMeters != null ? `${fmtArea(c.areaSquareMeters)} m²` : '无面积（开放链）';
    li.innerHTML = `
      <span class="swatch" style="background:${color}"></span>
      <span class="cid">${escapeHtml(c.id)} · ${labelKind(c)}</span>
      <span class="area">${area}</span>
      <span class="meta">${escapeHtml(c.group)} · ${c.objectIds.join(', ')} · ${c.points2d.length} 点 · ${c.kind === 'hole' ? `孔 → ${c.parentId}` : c.parentId ? `父 ${c.parentId}` : '顶层'}</span>
    `;
    li.addEventListener('click', () => selectContour(c.id));
    list.appendChild(li);
  });
}

function labelKind(c) {
  if (!c.closed) return '开放';
  if (c.kind === 'hole') return '孔洞';
  if (c.depth === 0) return '外轮廓';
  return `深度 ${c.depth}`;
}

function renderDiagnostics(result) {
  const box = el('diag-list');
  box.innerHTML = '';
  el('diag-count').textContent = String(result.diagnostics.length);
  if (result.diagnostics.length === 0) {
    box.innerHTML = '<div class="empty">没有诊断：剖面完整闭合。</div>';
    return;
  }
  for (const d of result.diagnostics) {
    const div = document.createElement('div');
    div.className = `diag ${d.severity}`;
    div.innerHTML = `<div class="code">${escapeHtml(d.code)} · ${escapeHtml(d.severity)}</div><div>${escapeHtml(d.message)}</div>`;
    box.appendChild(div);
  }
}

function setContours() {
  el('contour-list').innerHTML = '<li class="empty">尚无结果。</li>';
  el('contour-count').textContent = '0';
}
function setDiagnostics() {
  el('diag-list').innerHTML = '<div class="empty">尚无结果。</div>';
  el('diag-count').textContent = '0';
}

function selectContour(id) {
  if (!state.result) return;
  const c = state.result.contours.find((x) => x.id === id);
  if (!c) return;
  state.selectedId = id;
  document.querySelectorAll('.contour-item').forEach((li) => {
    li.classList.toggle('selected', li.dataset.contourId === id);
  });
  viewport.selectContour(id);
  if (el('chk-sources').checked) viewport.highlightFaces(c.faceRefs);
  else viewport.clearSelection();
  setDetail(c);
}

function setDetail(c) {
  const box = el('detail');
  if (!c) {
    box.innerHTML = '<div class="empty">选择一个轮廓查看交点、面积与来源面。</div>';
    return;
  }
  const plane = state.result?.plane;
  const pts = c.points3d
    .map((p, i) => `${String(i).padStart(3)}  ${fmt(p[0])}, ${fmt(p[1])}, ${fmt(p[2])}`)
    .join('\n');
  const faces = c.faceRefs.length
    ? c.faceRefs.map((f) => `<span class="chip hit">${escapeHtml(f.objectId)}·F${f.faceId}</span>`).join('')
    : '<span class="chip">无来源</span>';
  const segRows = (c.segments || [])
    .map(
      (s, i) =>
        `<div class="chip">e${i}: ${fmt(s.a[0])},${fmt(s.a[1])} → ${fmt(s.b[0])},${fmt(s.b[1])}</div>`
    )
    .join('');
  box.innerHTML = `
    <h3>${escapeHtml(c.id)} · ${labelKind(c)}</h3>
    <div class="kv">
      <span class="k">组</span><span class="v">${escapeHtml(c.group)}</span>
      <span class="k">对象</span><span class="v">${escapeHtml(c.objectIds.join(', '))}</span>
      <span class="k">闭合</span><span class="v">${c.closed ? '是' : '否（保留诊断）'}</span>
      <span class="k">环绕</span><span class="v">${c.orientation ?? '无'}</span>
      <span class="k">父轮廓</span><span class="v">${c.parentId ?? '无'}</span>
      <span class="k">有符号面积</span><span class="v">${c.signedArea != null ? fmt(c.signedArea) : '无'}</span>
      <span class="k">面积</span><span class="v">${c.areaSquareMeters != null ? `${fmtArea(c.areaSquareMeters)} m²` : '无'}</span>
      <span class="k">平面</span><span class="v">n=${fmtNormal(plane?.normal || state.normal)} d=${fmt(plane?.offset ?? state.offset)}</span>
    </div>
    <h3 style="font-size:11px;color:#8b98aa">交点（模型坐标 X,Y,Z）</h3>
    <div class="point-list">${pts || '（无）'}</div>
    <h3 style="font-size:11px;color:#8b98aa;margin-top:10px">来源面（点击“高亮原面”显示）</h3>
    <div class="face-chips">${faces}</div>
    <h3 style="font-size:11px;color:#8b98aa;margin-top:10px">逐段来源</h3>
    <div class="face-chips">${segRows || '<span class="chip">无分段</span>'}</div>
    <div style="margin-top:10px">
      <button class="btn ghost" id="btn-detail-focus">聚焦此轮廓</button>
    </div>
  `;
  el('btn-detail-focus')?.addEventListener('click', () => viewport.focusContour(c));
}

function updateExportButtons() {
  const ready = !!(state.result && state.frozen && !state.frozen.stale);
  for (const id of ['btn-export-json', 'btn-export-dxf', 'btn-export-csv']) {
    el(id).disabled = !ready;
  }
  el('export-note').textContent = ready
    ? `将导出冻结结果 ${state.frozen.result.resultId}（单位 ${state.unit.name}，平面 n=${fmtNormal(state.normal)} d=${fmt(state.offset)}）。`
    : '冻结后可导出；导出文件会写明实际单位与平面定义。';
}

function freeze() {
  if (!state.result) {
    showToast('当前没有可冻结的剖面。');
    return;
  }
  state.frozen = {
    result: state.result,
    frozenAt: new Date().toISOString(),
    plane: { normal: [...state.normal], offset: state.offset },
    sceneId: state.sceneId,
    sceneName: state.sceneName,
    stale: false,
  };
  setStatus('freeze-status', '已冻结', 'ok');
  el('freeze-info').textContent = `已冻结 ${state.result.resultId}\n平面 n=${fmtNormal(state.normal)} d=${fmt(state.offset)}\n单位 ${state.unit.name}（1 单位 = ${state.unit.metersPerUnit} m）\n该快照与后续平面移动断开。`;
  updateExportButtons();
}

function thaw() {
  state.frozen = null;
  setStatus('freeze-status', '未冻结', '');
  el('freeze-info').textContent = '移动平面后需要点击“冻结结果”才能锁定当前剖面用于导出与核对。';
  updateExportButtons();
}

function checkFrozenStillMatches() {
  if (!state.frozen) return;
  const p = state.frozen.plane;
  const sameOffset = Math.abs(p.offset - state.offset) <= 1e-9;
  const sameNormal =
    Math.abs(p.normal[0] - state.normal[0]) < 1e-12 &&
    Math.abs(p.normal[1] - state.normal[1]) < 1e-12 &&
    Math.abs(p.normal[2] - state.normal[2]) < 1e-12;
  if (!sameOffset || !sameNormal) {
    state.frozen.stale = true;
    setStatus('freeze-status', '冻结已过期', 'err');
    el('freeze-info').textContent = '平面已改变，冻结快照与当前剖切位置不一致；重新冻结或解除冻结。';
  } else if (state.frozen.stale) {
    state.frozen.stale = false;
    setStatus('freeze-status', '已冻结', 'ok');
  }
  updateExportButtons();
}

// ---------------------------------------------------------------- controls

el('btn-demo').addEventListener('click', () => {
  const name = el('fixture-select').value || 'demo-building.json';
  loadFixture(name).catch((e) => showToast(e.message));
});
el('btn-file').addEventListener('click', () => el('file-input').click());
el('file-input').addEventListener('change', async (e) => {
  const file = e.target.files?.[0];
  if (!file) return;
  try {
    const text = await file.text();
    const doc = JSON.parse(text);
    await adoptScene(doc, file.name);
  } catch (err) {
    showToast(`无法载入文件：${err.message}`);
  } finally {
    e.target.value = '';
  }
});

document.querySelectorAll('.seg').forEach((btn) => {
  btn.addEventListener('click', () => {
    const axis = btn.dataset.axis;
    if (axis === 'custom') {
      document.querySelectorAll('.seg').forEach((b) => b.classList.toggle('active', b === btn));
      el('custom-normal').style.display = '';
      return;
    }
    const n = { x: [1, 0, 0], y: [0, 1, 0], z: [0, 0, 1] }[axis];
    state.normal = n;
    syncNormalButtons();
    configureOffsetRange();
    // Keep the cut meaningful when switching plane direction: if the current
    // offset is outside the model's projected extent, move to its middle.
    const { min, max } = state.sliderRange;
    if (state.offset < min || state.offset > max) {
      state.offset = (min + max) / 2;
      syncOffsetControls(state.offset);
    }
    viewport.setPlane(state.normal, state.offset);
    checkFrozenStillMatches();
    computeSection();
  });
});

el('btn-apply-normal').addEventListener('click', () => {
  const n = [Number(el('nx').value) || 0, Number(el('ny').value) || 0, Number(el('nz').value) || 0];
  if (Math.hypot(...n) < 1e-9) {
    showToast('法向不能为零向量。');
    return;
  }
  state.normal = n;
  syncNormalButtons();
  configureOffsetRange();
  const { min, max } = state.sliderRange;
  if (state.offset < min || state.offset > max) {
    state.offset = (min + max) / 2;
    syncOffsetControls(state.offset);
  }
  viewport.setPlane(state.normal, state.offset);
  checkFrozenStillMatches();
  computeSection();
});

el('offset-slider').addEventListener('input', (e) => {
  state.offset = Number(e.target.value);
  syncOffsetControls(state.offset);
  viewport.setPlane(state.normal, state.offset);
  checkFrozenStillMatches();
  if (state.live) scheduleSection(50);
});

el('offset-number').addEventListener('change', (e) => {
  const v = Number(e.target.value);
  if (!Number.isFinite(v)) return;
  state.offset = clampOffset(v);
  syncOffsetControls(state.offset);
  viewport.setPlane(state.normal, state.offset);
  checkFrozenStillMatches();
  computeSection();
});

el('btn-section').addEventListener('click', () => computeSection());
el('btn-fit').addEventListener('click', () => viewport.frameBox(viewport.bounds?.box));

el('chk-section').addEventListener('change', (e) => viewport.setSectionVisible(e.target.checked));
el('chk-plane').addEventListener('change', (e) => viewport.setPlaneVisible(e.target.checked));
el('chk-sources').addEventListener('change', (e) => {
  if (!e.target.checked) viewport.clearSelection();
  else if (state.selectedId) {
    const c = state.result?.contours.find((x) => x.id === state.selectedId);
    if (c) viewport.highlightFaces(c.faceRefs);
  }
});
el('chk-live').addEventListener('change', (e) => { state.live = e.target.checked; });
el('opacity-slider').addEventListener('input', (e) => {
  const v = Number(e.target.value);
  el('opacity-value').textContent = v.toFixed(2);
  viewport.setModelOpacity(v);
});

el('btn-freeze').addEventListener('click', freeze);
el('btn-thaw').addEventListener('click', thaw);

el('btn-export-json').addEventListener('click', () => guardedExport(exportJson));
el('btn-export-dxf').addEventListener('click', () => guardedExport(exportDxf));
el('btn-export-csv').addEventListener('click', () => guardedExport(exportCsv));

function guardedExport(fn) {
  if (!state.frozen || state.frozen.stale) {
    showToast('请先冻结当前结果再导出。');
    return;
  }
  fn(state.frozen.result, state.sceneName, { frozenAt: state.frozen.frozenAt });
}

// ---------------------------------------------------------------- helpers

function setStatus(id, text, kind) {
  const node = el(id);
  node.textContent = text;
  node.classList.remove('ok', 'err', 'busy');
  if (kind) node.classList.add(kind);
}

let toastTimer = null;
function showToast(msg) {
  const t = el('toast');
  t.textContent = msg;
  t.classList.add('show');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => t.classList.remove('show'), 6000);
}

function fmt(n) {
  if (n == null || !Number.isFinite(Number(n))) return '—';
  const v = Number(n);
  if (Math.abs(v) >= 1e5 || (Math.abs(v) < 1e-3 && v !== 0)) return v.toExponential(4);
  return v.toFixed(3).replace(/\.?0+$/, '') || '0';
}
function fmtInput(n) {
  return Number.isFinite(Number(n)) ? String(Number(n)) : '';
}
function fmtArea(a) {
  if (!Number.isFinite(a)) return '—';
  return a.toFixed(a < 1 ? 4 : 2);
}
function escapeHtml(s) {
  return String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
}

// Expose a tiny debug surface for the automated UI tests.
window.__sectionApp = {
  state,
  computeSection,
  selectContour,
  freeze,
  thaw,
};
