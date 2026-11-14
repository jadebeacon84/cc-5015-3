const API_BASE = window.__SECTION_API__ || '';

async function readJson(res) {
  const text = await res.text();
  let body;
  try {
    body = text ? JSON.parse(text) : {};
  } catch {
    throw new Error(`服务返回了非 JSON 响应（HTTP ${res.status}）`);
  }
  if (!res.ok || body.ok === false) {
    const err = body.error || {};
    const details = (err.details || []).map((d) => `${d.code}: ${d.message}`).join('\n');
    const e = new Error(details || err.message || `请求失败（HTTP ${res.status}）`);
    e.code = err.code || `http_${res.status}`;
    throw e;
  }
  return body;
}

export async function health() {
  const res = await fetch(`${API_BASE}/api/health`);
  return readJson(res);
}

export async function loadScene(scene, signal) {
  const res = await fetch(`${API_BASE}/api/scene`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(scene),
    signal,
  });
  return readJson(res);
}

/**
 * Live section query. Sends only the plane when the scene is already loaded;
 * this keeps drag updates small enough for interactive use.
 */
export async function section({ sceneId, scene, normal, offset, includeSegments = true }, signal) {
  const payload = {
    normal,
    offset,
    includeSegments,
  };
  if (sceneId) payload.sceneId = sceneId;
  else payload.scene = scene;
  const res = await fetch(`${API_BASE}/api/section`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
    signal,
  });
  return readJson(res);
}

export async function loadFixture(name = 'demo-building.json') {
  const res = await fetch(`./fixtures/${name}`, { cache: 'no-store' });
  if (!res.ok) throw new Error(`无法加载示例场景 ${name}（HTTP ${res.status}）`);
  return res.json();
}
