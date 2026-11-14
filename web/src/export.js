/**
 * Export helpers. All exports describe the actual unit and the exact plane
 * definition used for the measurement, so a downstream reader can reproduce
 * the section rather than treating it as a pretty picture.
 */

function download(filename, text, mime = 'application/json') {
  const blob = new Blob([text], { type: `${mime};charset=utf-8` });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 0);
}

export function exportJson(result, sceneName, frozenInfo) {
  const doc = {
    format: 'archisection.export/1',
    exportedAt: new Date().toISOString(),
    scene: sceneName,
    frozen: frozenInfo,
    unit: result.unit,
    coordinateSystem: result.coordinateSystem,
    plane: result.plane,
    tolerance: result.tolerance,
    summary: {
      resultId: result.resultId,
      contours: result.contours.length,
      closedContours: result.stats.closedContours,
      openChains: result.stats.openChains,
      holes: result.stats.holes,
      caps: result.caps.length,
      capAreaSquareMeters: round(result.caps.reduce((a, c) => a + c.areaSquareMeters, 0), 6),
      sourceFaces: dedupeFaces(result.contours.flatMap((c) => c.faceRefs)).length,
    },
    contours: result.contours,
    caps: result.caps,
    diagnostics: result.diagnostics,
  };
  download(`${safeName(sceneName)}-section-${result.resultId}.json`, JSON.stringify(doc, null, 2));
}

/**
 * DXF with all section geometry placed on the cut plane, in model units.
 * A header comment records the plane so the drawing remains measurable.
 * Coordinates are (u,v) on the plane plus the 3D coordinates echoed per LINE.
 */
export function exportDxf(result, sceneName, frozenInfo) {
  const out = [];
  out.push('0'); out.push('SECTION');
  out.push('2'); out.push('HEADER');
  out.push('9'); out.push('$ACADVER'); out.push('1'); out.push('AC1015');
  out.push('0'); out.push('ENDSEC');
  out.push('0'); out.push('SECTION');
  out.push('2'); out.push('ENTITIES');

  for (const c of result.contours) {
    const pts = c.points2d;
    out.push('0'); out.push('POLYLINE');
    out.push('8'); out.push(`CONTOUR_${c.kind.toUpperCase()}`);
    out.push('66'); out.push('1');
    out.push('70'); out.push(c.closed ? '1' : '0');
    out.push('1001'); out.push('ARCHISECTION');
    out.push('1000'); out.push(`id=${c.id}`);
    out.push('1000'); out.push(`area_m2=${c.areaSquareMeters ?? 'open'}`);
    for (const p of pts) {
      out.push('0'); out.push('VERTEX');
      out.push('8'); out.push(`CONTOUR_${c.kind.toUpperCase()}`);
      out.push('10'); out.push(fmt(p[0]));
      out.push('20'); out.push(fmt(p[1]));
      out.push('30'); out.push('0');
    }
    out.push('0'); out.push('SEQEND');
    out.push('8'); out.push(`CONTOUR_${c.kind.toUpperCase()}`);
  }
  out.push('0'); out.push('ENDSEC');
  out.push('0'); out.push('EOF');

  const header = [
    `archisection export; scene=${sceneName}`,
    `unit=${result.unit.name}; metersPerUnit=${result.unit.metersPerUnit}`,
    `plane: normal=(${result.plane.normal.map(fmt).join(',')}) offset=${fmt(result.plane.offset)}`,
    `plane origin=(${result.plane.origin.map(fmt).join(',')})`,
    `basisU=(${result.plane.basisU.map(fmt).join(',')}) basisV=(${result.plane.basisV.map(fmt).join(',')})`,
    'DXF X = plane u, DXF Y = plane v. Multiply by metersPerUnit for metres.',
    frozenInfo ? `frozen: ${frozenInfo.frozenAt}` : 'frozen: no',
    '',
  ].join('\n');

  // DXF comments start with 999 and are allowed in ENTITIES.
  const commentLines = [];
  for (const line of header.split('\n')) {
    if (!line) continue;
    commentLines.push('999', line);
  }
  const idx = out.indexOf('ENTITIES') + 1;
  out.splice(idx, 0, ...commentLines);

  download(`${safeName(sceneName)}-section-${result.resultId}.dxf`, out.join('\n'), 'application/dxf');
}

export function exportCsv(result, sceneName, frozenInfo) {
  const header = [
    'record', 'scene', 'result_id', 'unit', 'meters_per_unit',
    'plane_normal_x', 'plane_normal_y', 'plane_normal_z', 'plane_offset',
    'contour_id', 'group', 'kind', 'closed', 'parent_id', 'orientation',
    'signed_area_model_units2', 'area_m2', 'point_index',
    'x_model', 'y_model', 'z_model', 'u_plane', 'v_plane',
  ].join(',');
  const rows = [];
  const p = result.plane;
  for (const c of result.contours) {
    c.points3d.forEach((pt, i) => {
      const uv = c.points2d[i] || ['', ''];
      rows.push([
        'contour', csv(sceneName), result.resultId, result.unit.name, result.unit.metersPerUnit,
        p.normal[0], p.normal[1], p.normal[2], p.offset,
        c.id, csv(c.group), c.kind, c.closed, c.parentId ?? '', c.orientation ?? '',
        c.signedArea ?? '', c.areaSquareMeters ?? '',
        i, pt[0], pt[1], pt[2], uv[0], uv[1],
      ].join(','));
    });
  }
  for (const cap of result.caps) {
    rows.push([
      'cap', csv(sceneName), result.resultId, result.unit.name, result.unit.metersPerUnit,
      p.normal[0], p.normal[1], p.normal[2], p.offset,
      cap.id, csv(cap.group), 'cap', true, cap.outerContourId, '',
      '', cap.areaSquareMeters, '', '', '', '', '', '',
    ].join(','));
  }
  const note = [
    `# archisection export`,
    `# unit=${result.unit.name} metersPerUnit=${result.unit.metersPerUnit}`,
    `# plane normal=(${p.normal.map(fmt).join(',')}) offset=${fmt(p.offset)} origin=(${p.origin.map(fmt).join(',')})`,
    `# X/Y/Z are model units; u/v are plane coordinates, multiply by metersPerUnit for metres.`,
    `# result_id=${result.resultId} closed_contours=${result.stats.closedContours} open_chains=${result.stats.openChains}`,
    `# frozen=${frozenInfo ? frozenInfo.frozenAt : 'no'}`,
  ].join('\n');
  download(
    `${safeName(sceneName)}-section-${result.resultId}.csv`,
    `${note}\n${header}\n${rows.join('\n')}\n`,
    'text/csv'
  );
}

function dedupeFaces(refs) {
  const seen = new Set();
  return refs.filter((r) => {
    const k = `${r.objectId}|${r.faceId}|${r.triangleIndex}`;
    if (seen.has(k)) return false;
    seen.add(k);
    return true;
  });
}

function safeName(s) {
  return String(s).replace(/[^\w一-龥-]+/g, '_').slice(0, 60) || 'scene';
}

function fmt(n) {
  return Number(n).toPrecision(12).replace(/\.?0+$/, '') || '0';
}

function round(n, d) {
  const f = 10 ** d;
  return Math.round(n * f) / f;
}

function csv(v) {
  const s = String(v ?? '');
  return /[",\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
}
