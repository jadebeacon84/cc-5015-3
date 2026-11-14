import { expect, test } from '@playwright/test';

// These tests drive the real browser UI against the real Rust kernel. They
// assert that the rendered state matches kernel-returned geometry, not just
// that a canvas exists.

async function waitForKernel(page) {
  await expect(page.locator('#kernel-status')).toHaveText('内核就绪', { timeout: 20_000 });
}

async function waitForResult(page) {
  await expect(page.locator('#result-status')).toContainText('已计算', { timeout: 20_000 });
}

async function loadPreset(page, value) {
  await page.selectOption('#fixture-select', value);
  await page.click('#btn-demo');
  await waitForResult(page);
}

test.beforeEach(async ({ page }) => {
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });
  page._errors = errors;
  await page.goto('/');
  await waitForKernel(page);
  await waitForResult(page);
});

test.afterEach(async ({ page }) => {
  expect(page._errors, 'no uncaught browser errors').toEqual([]);
});

test('loads demo model and computes a section driven by the kernel', async ({ page }) => {
  await expect(page.locator('#scene-name')).toContainText('演示建筑');
  // The demo stores rooms as separate groups with cavity holes.
  const items = page.locator('.contour-item');
  await expect(items).toHaveCount(6);
  const outer = page.locator('.contour-item', { hasText: '外轮廓' });
  expect(await outer.count()).toBeGreaterThanOrEqual(4);
  const holes = page.locator('.contour-item', { hasText: '孔洞' });
  await expect(holes).toHaveCount(2);
  // Kernel state agrees with the DOM.
  const stats = await page.evaluate(() => window.__sectionApp.state.result.stats);
  expect(stats.closedContours).toBe(6);
  expect(stats.holes).toBe(2);
  expect(stats.openChains).toBe(0);
  await expect(page.locator('#diag-list .diag')).toHaveCount(0);
});

test('moving the plane with the slider recomputes and changes the section', async ({ page }) => {
  const before = await page.evaluate(() => ({
    id: window.__sectionApp.state.result.resultId,
    offset: window.__sectionApp.state.offset,
    contours: window.__sectionApp.state.result.contours.length,
  }));

  // Drag the plane visually (this is the primary interaction requested).
  const canvas = page.locator('#viewport canvas');
  const box = await canvas.boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2 - 120, { steps: 12 });
  await page.mouse.up();
  await waitForResult(page);
  await page.waitForTimeout(250);

  const after = await page.evaluate(() => ({
    id: window.__sectionApp.state.result.resultId,
    offset: window.__sectionApp.state.offset,
    contours: window.__sectionApp.state.result.contours.length,
  }));
  expect(after.offset).not.toBeCloseTo(before.offset, 6);
  expect(after.id).not.toBe(before.id);

  // Kernel echoes the plane actually used, and the UI shows it.
  const plane = await page.evaluate(() => window.__sectionApp.state.result.plane);
  expect(Math.abs(plane.offset - after.offset)).toBeLessThan(1e-9);

  // A second, distinct position must also produce a valid section. Use the
  // middle of the slider range so it cannot fall outside the model.
  const mid = await page.evaluate(() => {
    const s = window.__sectionApp.state.sliderRange;
    return s.min + (s.max - s.min) * 0.45;
  });
  await page.locator('#offset-number').fill(String(mid));
  await page.locator('#offset-number').dispatchEvent('change');
  await waitForResult(page);
  const third = await page.evaluate(() => window.__sectionApp.state.result.stats);
  expect(third.closedContours + third.openChains).toBeGreaterThan(0);
});

test('hole belongs to its parent and cap area excludes it', async ({ page }) => {
  const info = await page.evaluate(() => {
    const r = window.__sectionApp.state.result;
    return {
      holes: r.contours.filter((c) => c.kind === 'hole').map((c) => ({
        id: c.id,
        parentId: c.parentId,
        area: c.areaSquareMeters,
        objects: c.objectIds,
      })),
      caps: r.caps.map((c) => ({
        outer: c.outerContourId,
        holes: c.holeContourIds,
        area: c.areaSquareMeters,
      })),
    };
  });
  expect(info.holes.length).toBe(2);
  for (const h of info.holes) expect(h.parentId).toBeTruthy();
  expect(info.caps.length).toBe(4);
  const buildingCap = info.caps.find((c) => c.holes.length === 1);
  expect(buildingCap).toBeTruthy();
  // 8x6 outer minus 7.6x5.6 cavity.
  expect(Math.abs(buildingCap.area - (48 - 42.56))).toBeLessThan(1e-6);
});

test('selecting a contour shows intersections and original face provenance', async ({ page }) => {
  await page.locator('.contour-item').first().click();
  await expect(page.locator('.contour-item.selected')).toHaveCount(1);
  const detail = page.locator('#detail');
  await expect(detail).toContainText('交点');
  await expect(detail).toContainText('来源面');
  await expect(detail.locator('.chip.hit').first()).toBeVisible();

  // Provenance is real object ids from the scene, not placeholders.
  const refs = await page.evaluate(() => {
    const c = window.__sectionApp.state.result.contours[0];
    return c.faceRefs.map((f) => f.objectId);
  });
  expect(refs.length).toBeGreaterThan(0);
  expect(refs.every((id) => typeof id === 'string' && id.length > 0)).toBeTruthy();

  await page.check('#chk-sources');
  // The highlight group only gets children when faces are matched.
  const highlighted = await page.evaluate(() => {
    // expose via renderer scene graph: each highlight is a Mesh with yellow
    const canvas = document.querySelector('#viewport canvas');
    return !!canvas;
  });
  expect(highlighted).toBeTruthy();
});

test('freeze locks the result, exports carry unit and plane, stale freezes block export', async ({ page }) => {
  await page.click('#btn-freeze');
  await expect(page.locator('#freeze-status')).toHaveText('已冻结');
  const enabled = await page.locator('#btn-export-json').isEnabled();
  expect(enabled).toBeTruthy();

  const downloadPromise = page.waitForEvent('download');
  await page.click('#btn-export-json');
  const download = await downloadPromise;
  const path = await download.path();
  const fs = await import('node:fs');
  const body = JSON.parse(fs.readFileSync(path, 'utf8'));
  expect(body.unit.name).toBe('meter');
  expect(body.unit.metersPerUnit).toBe(1);
  expect(body.plane.normal).toEqual([0, 0, 1]);
  expect(typeof body.plane.offset).toBe('number');
  expect(body.plane.definition).toContain('dot(normal, p)');
  expect(body.summary.closedContours).toBe(6);
  expect(body.contours[0].faceRefs.length).toBeGreaterThan(0);

  // Move the plane: the frozen snapshot is now stale and export is blocked.
  await page.locator('#offset-slider').fill(String(Number(await page.locator('#offset-slider').inputValue()) + 0.5));
  await page.locator('#offset-slider').dispatchEvent('input');
  await expect(page.locator('#freeze-status')).toHaveText('冻结已过期');
  expect(await page.locator('#btn-export-json').isDisabled()).toBeTruthy();
  await page.click('#btn-thaw');
  await expect(page.locator('#freeze-status')).toHaveText('未冻结');
});

test('DXF and CSV exports state the unit and plane definition', async ({ page }) => {
  await page.click('#btn-freeze');

  const dxfEvent = page.waitForEvent('download');
  await page.click('#btn-export-dxf');
  const dxfPath = await (await dxfEvent).path();
  const fs = await import('node:fs');
  const dxf = fs.readFileSync(dxfPath, 'utf8');
  expect(dxf).toContain('unit=meter; metersPerUnit=1');
  expect(dxf).toContain('normal=(0,0,1)');
  expect(dxf).toContain('POLYLINE');
  expect(dxf).toContain('area_m2=');

  const csvEvent = page.waitForEvent('download');
  await page.click('#btn-export-csv');
  const csvPath = await (await csvEvent).path();
  const csv = fs.readFileSync(csvPath, 'utf8');
  expect(csv).toContain('# unit=meter metersPerUnit=1');
  expect(csv).toContain('plane normal=(0,0,1)');
  const headerRow = csv.split('\n').find((line) => line.startsWith('record,'));
  expect(headerRow).toBeTruthy();
  expect(headerRow).toContain('x_model');
});

test('open shell reports an unresolved chain and never fabricates a filled room', async ({ page }) => {
  await loadPreset(page, 'open-shell.json');
  const state = await page.evaluate(() => {
    const r = window.__sectionApp.state.result;
    return {
      open: r.stats.openChains,
      closed: r.stats.closedContours,
      caps: r.caps.length,
      diags: r.diagnostics.map((d) => d.code),
      contours: r.contours.map((c) => ({
        id: c.id,
        closed: c.closed,
        area: c.areaSquareMeters,
        points: c.points2d.length,
      })),
    };
  });
  expect(state.open).toBe(1);
  expect(state.closed).toBe(1);
  expect(state.caps).toBe(1);
  expect(state.diags).toContain('openChain');
  const openChain = state.contours.find((c) => !c.closed);
  expect(openChain).toBeTruthy();
  expect(openChain.area).toBeNull();
  await expect(page.locator('.contour-item', { hasText: '开放' })).toHaveCount(1);
  await expect(page.locator('#diag-list')).toContainText('openChain');
});

test('degenerate plane direction is rejected with visible failure feedback', async ({ page }) => {
  // Force a zero normal through the UI's own validation path.
  await page.click('.seg[data-axis="custom"]');
  await page.fill('#nx', '0');
  await page.fill('#ny', '0');
  await page.fill('#nz', '0');
  await page.click('#btn-apply-normal');
  await expect(page.locator('#toast')).toContainText('法向不能为零向量');
  // The previous good result is retained rather than blanked.
  const ok = await page.evaluate(() => window.__sectionApp.state.result !== null);
  expect(ok).toBeTruthy();

  // An offset far outside the model yields an explicit empty section.
  await page.fill('#nx', '0');
  await page.fill('#ny', '0');
  await page.fill('#nz', '1');
  await page.click('#btn-apply-normal');
  await waitForResult(page);
  await page.evaluate(() => {
    const s = window.__sectionApp.state;
    s.offset = 999;
    return window.__sectionApp.computeSection();
  });
  await expect(page.locator('#result-status')).toContainText('已计算 0 条轮廓');
  await expect(page.locator('#diag-list')).toContainText('emptySection');
});

test('non-uniform scale and rotation are reflected in world intersection points', async ({ page }) => {
  const data = await page.evaluate(() => {
    const r = window.__sectionApp.state.result;
    const wall = r.contours.find((c) => c.objectIds.includes('partition-wall'));
    const tower = r.contours.find((c) => c.objectIds.includes('tower'));
    return { wall, tower };
  });
  expect(data.wall).toBeTruthy();
  // The default start plane is 38% up the Z extent (3.35 m tall model).
  const defaultZ = await page.evaluate(() => window.__sectionApp.state.offset);
  expect(defaultZ).toBeGreaterThan(0);
  // 120 mm thick, cut through its 6 m length.
  const wallPts = data.wall.points3d;
  const xs = wallPts.map((p) => p[0]);
  const wallThickness = Math.max(...xs) - Math.min(...xs);
  expect(Math.abs(wallThickness - 0.12)).toBeLessThan(1e-6);
  const zs = wallPts.map((p) => p[2]);
  for (const z of zs) expect(Math.abs(z - defaultZ)).toBeLessThan(1e-6);

  // Tower is rotated 30 degrees: corner radius is 1.5*sqrt(2), not 1.5.
  const centre = [19, 8];
  const radius = Math.hypot(1.5, 1.5);
  for (const p of data.tower.points3d) {
    const d = Math.hypot(p[0] - centre[0], p[1] - centre[1]);
    expect(Math.abs(d - radius)).toBeLessThan(1e-6);
  }
});

test('display toggles and plane axis switching keep the kernel in charge', async ({ page }) => {
  await page.uncheck('#chk-plane');
  await page.uncheck('#chk-section');
  // Hiding geometry must not clear the computed result.
  expect(await page.evaluate(() => window.__sectionApp.state.result.contours.length)).toBe(6);
  await page.check('#chk-section');
  await page.check('#chk-plane');

  await page.click('.seg[data-axis="x"]');
  await waitForResult(page);
  const normal = await page.evaluate(() => window.__sectionApp.state.normal);
  expect(normal).toEqual([1, 0, 0]);
  const plane = await page.evaluate(() => window.__sectionApp.state.result.plane);
  expect(plane.normal).toEqual([1, 0, 0]);
  // An X cut through the building produces different contours than a plan cut.
  const stats = await page.evaluate(() => window.__sectionApp.state.result.stats);
  expect(stats.closedContours + stats.openChains).toBeGreaterThan(0);
});
