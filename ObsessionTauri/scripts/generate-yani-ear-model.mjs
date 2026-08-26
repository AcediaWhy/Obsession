import { mkdir, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const outputPath = resolve(scriptDir, "../public/yani/ear-surface.gltf");
const rows = 64;
const columns = 48;

const outerProfile = [
  [0, 0.56], [0.12, 0.545], [0.3, 0.475], [0.5, 0.365],
  [0.7, 0.235], [0.86, 0.115], [0.96, 0.052], [1, 0.035],
];
const innerProfile = [
  [0, 0.43], [0.12, 0.415], [0.3, 0.365], [0.5, 0.285],
  [0.7, 0.185], [0.86, 0.09], [0.96, 0.04], [1, 0.024],
];

function clamp(value, low = 0, high = 1) {
  return Math.max(low, Math.min(high, value));
}

function smoothstep(edge0, edge1, value) {
  const t = clamp((value - edge0) / Math.max(1e-6, edge1 - edge0));
  return t * t * (3 - 2 * t);
}

function sampleHermite(points, value) {
  let index = 0;
  while (index < points.length - 2 && value > points[index + 1][0]) index += 1;
  const [x0, y0] = points[index];
  const [x1, y1] = points[index + 1];
  const previous = points[Math.max(0, index - 1)];
  const next = points[Math.min(points.length - 1, index + 2)];
  const tangent0 = (y1 - previous[1]) / Math.max(1e-6, x1 - previous[0]);
  const tangent1 = (next[1] - y0) / Math.max(1e-6, next[0] - x0);
  const t = clamp((value - x0) / Math.max(1e-6, x1 - x0));
  const t2 = t * t;
  const t3 = t2 * t;
  const span = x1 - x0;
  return Math.max(0.006,
    (2 * t3 - 3 * t2 + 1) * y0
    + (t3 - 2 * t2 + t) * tangent0 * span
    + (-2 * t3 + 3 * t2) * y1
    + (t3 - t2) * tangent1 * span);
}

function frontPoint(u01, v) {
  const signed = u01 * 2 - 1;
  const outerWidth = sampleHermite(outerProfile, v);
  const innerWidth = sampleHermite(innerProfile, v);
  const tipSkew = -0.058 * smoothstep(0.7, 1, v);
  const x = (signed < 0 ? signed * outerWidth : signed * innerWidth) + tipSkew;
  const arch = Math.pow(Math.sin(clamp(v) * Math.PI), 0.72);
  const edge = Math.abs(signed);
  const lateralBias = signed < 0 ? 1.06 : 0.88;
  const cup = -Math.pow(1 - edge, 1.42) * arch * 0.205 * lateralBias;
  const roundedRim = Math.pow(edge, 3.2) * (0.042 + arch * (signed < 0 ? 0.092 : 0.066));
  const basalConcha = -Math.exp(-Math.pow((v - 0.18) / 0.15, 2))
    * Math.pow(1 - edge, 1.3) * 0.045;
  const forwardTip = smoothstep(0.78, 1, v) * (0.072 - edge * 0.018);
  const helicalWarp = Math.sin(v * 4.1 + signed * 1.8) * arch * (1 - edge) * 0.008;
  const y = -0.55 + v * 1.46
    - Math.pow(1 - edge, 1.8) * Math.pow(1 - v, 2) * 0.038
    + signed * smoothstep(0.82, 1, v) * 0.012;
  return [x, y, roundedRim + cup + basalConcha + forwardTip + helicalWarp];
}

function pushPoint(target, point) {
  target.push(point[0], point[1], point[2]);
}

function computeNormals(positions, indices) {
  const normals = new Float32Array(positions.length);
  for (let index = 0; index < indices.length; index += 3) {
    const ia = indices[index] * 3;
    const ib = indices[index + 1] * 3;
    const ic = indices[index + 2] * 3;
    const abx = positions[ib] - positions[ia];
    const aby = positions[ib + 1] - positions[ia + 1];
    const abz = positions[ib + 2] - positions[ia + 2];
    const acx = positions[ic] - positions[ia];
    const acy = positions[ic + 1] - positions[ia + 1];
    const acz = positions[ic + 2] - positions[ia + 2];
    const nx = aby * acz - abz * acy;
    const ny = abz * acx - abx * acz;
    const nz = abx * acy - aby * acx;
    for (const offset of [ia, ib, ic]) {
      normals[offset] += nx;
      normals[offset + 1] += ny;
      normals[offset + 2] += nz;
    }
  }
  for (let offset = 0; offset < normals.length; offset += 3) {
    const length = Math.hypot(normals[offset], normals[offset + 1], normals[offset + 2]) || 1;
    normals[offset] /= length;
    normals[offset + 1] /= length;
    normals[offset + 2] /= length;
  }
  return normals;
}

function gridIndices(gridRows, gridColumns, reverse = false) {
  const indices = [];
  const vertexIndex = (row, column) => row * (gridColumns + 1) + column;
  for (let row = 0; row < gridRows; row += 1) {
    for (let column = 0; column < gridColumns; column += 1) {
      const a = vertexIndex(row, column);
      const b = vertexIndex(row, column + 1);
      const c = vertexIndex(row + 1, column);
      const d = vertexIndex(row + 1, column + 1);
      if (reverse) indices.push(a, c, b, b, c, d);
      else indices.push(a, b, c, b, d, c);
    }
  }
  return indices;
}

function createFront() {
  const positions = [];
  const uvs = [];
  for (let row = 0; row <= rows; row += 1) {
    const v = row / rows;
    for (let column = 0; column <= columns; column += 1) {
      const u = column / columns;
      pushPoint(positions, frontPoint(u, v));
      uvs.push(u, v);
    }
  }
  const indices = gridIndices(rows, columns);
  return { positions, normals: Array.from(computeNormals(positions, indices)), uvs, indices };
}

function createBack(front) {
  const positions = [];
  for (let vertex = 0; vertex < front.positions.length / 3; vertex += 1) {
    const offset = vertex * 3;
    const v = front.uvs[vertex * 2 + 1];
    const u = front.uvs[vertex * 2];
    const thickness = 0.014 + (1 - smoothstep(0.55, 1, v)) * 0.021
      + Math.abs(u - 0.5) * 0.004;
    positions.push(
      front.positions[offset] - front.normals[offset] * thickness,
      front.positions[offset + 1] - front.normals[offset + 1] * thickness,
      front.positions[offset + 2] - front.normals[offset + 2] * thickness,
    );
  }
  const indices = gridIndices(rows, columns, true);
  return { positions, normals: Array.from(computeNormals(positions, indices)), uvs: [...front.uvs], indices };
}

function createRim(front, back) {
  const vertexIndex = (row, column) => row * (columns + 1) + column;
  const boundary = [];
  for (let column = 0; column <= columns; column += 1) boundary.push(vertexIndex(0, column));
  for (let row = 1; row <= rows; row += 1) boundary.push(vertexIndex(row, columns));
  for (let column = columns - 1; column >= 0; column -= 1) boundary.push(vertexIndex(rows, column));
  for (let row = rows - 1; row > 0; row -= 1) boundary.push(vertexIndex(row, 0));

  const positions = [];
  const uvs = [];
  const indices = [];
  boundary.forEach((sourceIndex) => {
    const p = sourceIndex * 3;
    const uv = sourceIndex * 2;
    positions.push(
      front.positions[p], front.positions[p + 1], front.positions[p + 2],
      back.positions[p], back.positions[p + 1], back.positions[p + 2],
    );
    uvs.push(front.uvs[uv], front.uvs[uv + 1], front.uvs[uv], front.uvs[uv + 1]);
  });
  for (let index = 0; index < boundary.length; index += 1) {
    const next = (index + 1) % boundary.length;
    const frontCurrent = index * 2;
    const backCurrent = frontCurrent + 1;
    const frontNext = next * 2;
    const backNext = frontNext + 1;
    indices.push(frontCurrent, frontNext, backCurrent, frontNext, backNext, backCurrent);
  }
  return { positions, normals: Array.from(computeNormals(positions, indices)), uvs, indices };
}

function pointNormal(u, v) {
  const step = 0.002;
  const left = frontPoint(clamp(u - step), v);
  const right = frontPoint(clamp(u + step), v);
  const down = frontPoint(u, clamp(v - step));
  const up = frontPoint(u, clamp(v + step));
  const ux = right[0] - left[0];
  const uy = right[1] - left[1];
  const uz = right[2] - left[2];
  const vx = up[0] - down[0];
  const vy = up[1] - down[1];
  const vz = up[2] - down[2];
  const nx = uy * vz - uz * vy;
  const ny = uz * vx - ux * vz;
  const nz = ux * vy - uy * vx;
  const length = Math.hypot(nx, ny, nz) || 1;
  return [nx / length, ny / length, nz / length];
}

function createBasalFold() {
  const foldRows = 18;
  const foldColumns = 12;
  const positions = [];
  const uvs = [];
  for (let row = 0; row <= foldRows; row += 1) {
    const rowT = row / foldRows;
    const v = 0.075 + rowT * 0.31;
    for (let column = 0; column <= foldColumns; column += 1) {
      const across = column / foldColumns;
      const u = 0.018 + across * (0.31 + rowT * 0.045);
      const point = frontPoint(u, v);
      const normal = pointNormal(u, v);
      const lip = Math.pow(across, 1.7) * (0.055 + Math.sin(rowT * Math.PI) * 0.038);
      const overlap = Math.sin(across * Math.PI) * Math.sin(rowT * Math.PI) * 0.028;
      positions.push(
        point[0] + normal[0] * (0.014 + lip) + overlap * 0.18,
        point[1] + normal[1] * (0.014 + lip) + overlap * 0.22,
        point[2] + normal[2] * (0.014 + lip) + overlap,
      );
      uvs.push(u, v);
    }
  }
  const indices = gridIndices(foldRows, foldColumns);
  return { positions, normals: Array.from(computeNormals(positions, indices)), uvs, indices };
}

const surfaces = [["YaniEarFront", createFront()]];
const front = surfaces[0][1];
const back = createBack(front);
const rim = createRim(front, back);
const fold = createBasalFold();
surfaces.push(["YaniEarBack", back], ["YaniEarRim", rim], ["YaniEarFold", fold]);

const chunks = [];
const bufferViews = [];
const accessors = [];
let byteLength = 0;

function appendView(array, target) {
  const padding = (4 - (byteLength % 4)) % 4;
  if (padding) {
    chunks.push(Buffer.alloc(padding));
    byteLength += padding;
  }
  const buffer = Buffer.from(array.buffer, array.byteOffset, array.byteLength);
  const viewIndex = bufferViews.length;
  bufferViews.push({ buffer: 0, byteOffset: byteLength, byteLength: buffer.length, target });
  chunks.push(buffer);
  byteLength += buffer.length;
  return viewIndex;
}

function positionBounds(array) {
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  for (let offset = 0; offset < array.length; offset += 3) {
    for (let axis = 0; axis < 3; axis += 1) {
      min[axis] = Math.min(min[axis], array[offset + axis]);
      max[axis] = Math.max(max[axis], array[offset + axis]);
    }
  }
  return { min, max };
}

function addAccessor(array, componentType, type, target, bounds) {
  const view = appendView(array, target);
  const itemSize = type === "VEC3" ? 3 : type === "VEC2" ? 2 : 1;
  accessors.push({ bufferView: view, componentType, count: array.length / itemSize, type, ...(bounds ?? {}) });
  return accessors.length - 1;
}

const meshes = [];
const nodes = [];
let totalVertices = 0;
let totalTriangles = 0;
for (const [name, surface] of surfaces) {
  const positions = new Float32Array(surface.positions);
  const normals = new Float32Array(surface.normals);
  const uvs = new Float32Array(surface.uvs);
  const indices = new Uint16Array(surface.indices);
  totalVertices += positions.length / 3;
  totalTriangles += indices.length / 3;
  const positionAccessor = addAccessor(positions, 5126, "VEC3", 34962, positionBounds(positions));
  const normalAccessor = addAccessor(normals, 5126, "VEC3", 34962);
  const uvAccessor = addAccessor(uvs, 5126, "VEC2", 34962, { min: [0, 0], max: [1, 1] });
  const indexAccessor = addAccessor(indices, 5123, "SCALAR", 34963, { min: [0], max: [positions.length / 3 - 1] });
  meshes.push({
    name,
    primitives: [{
      attributes: { POSITION: positionAccessor, NORMAL: normalAccessor, TEXCOORD_0: uvAccessor },
      indices: indexAccessor,
      mode: 4,
    }],
  });
  nodes.push({ name, mesh: meshes.length - 1 });
}

const binary = Buffer.concat(chunks, byteLength);
const gltf = {
  asset: { version: "2.0", generator: "Obsession anatomical Yani ear generator" },
  scene: 0,
  scenes: [{ nodes: nodes.map((_, index) => index) }],
  nodes,
  meshes,
  buffers: [{ byteLength: binary.length, uri: `data:application/octet-stream;base64,${binary.toString("base64")}` }],
  bufferViews,
  accessors,
  extras: {
    license: "Original procedural asset for Obsession / Yani Neko",
    topology: "low outward anime wedge with physical thickness and a separate basal cartilage fold",
    rows,
    columns,
  },
};

await mkdir(dirname(outputPath), { recursive: true });
await writeFile(outputPath, `${JSON.stringify(gltf)}\n`, "utf8");
console.log(`Generated ${outputPath} (${totalVertices} vertices, ${totalTriangles} triangles, ${surfaces.length} surfaces)`);
