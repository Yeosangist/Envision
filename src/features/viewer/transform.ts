
/** CSS affine transforms use column vectors; `multiply(a, b)` applies `b`, then `a`. */
export type Matrix = readonly [number, number, number, number, number, number];

export const IDENTITY: Matrix = [1, 0, 0, 1, 0, 0];

export const MIN_SCALE = 1;
export const MAX_SCALE = 8;
export const ZOOM_STEP = 1.15;
/** How far the view may travel, per axis, before a pan is contained. */
export const PAN_LIMIT = 2000;

/** The epsilon used to decide whether a scale is "still at fit". */
export const FIT_EPSILON = 1e-6;

/** `a · b`: apply `b` first, then `a`. */
export function multiply(a: Matrix, b: Matrix): Matrix {
  return [
    a[0] * b[0] + a[2] * b[1],
    a[1] * b[0] + a[3] * b[1],
    a[0] * b[2] + a[2] * b[3],
    a[1] * b[2] + a[3] * b[3],
    a[0] * b[4] + a[2] * b[5] + a[4],
    a[1] * b[4] + a[3] * b[5] + a[5],
  ];
}

/** Translate by `(x, y)` in the space the matrix maps into. */
export const translate = (x: number, y: number): Matrix => [1, 0, 0, 1, x, y];

export const scaleAbout = (pointX: number, pointY: number, k: number): Matrix => [
  k,
  0,
  0,
  k,
  pointX * (1 - k),
  pointY * (1 - k),
];

/** Scale factor of the accumulated matrix. */
export function scaleOf(m: Matrix): number {
  return Math.sqrt(Math.abs(m[0] * m[3] - m[1] * m[2])) || 1;
}

/** Map a point through the matrix, as the browser would when rendering it. */
export function applyToPoint(m: Matrix, x: number, y: number): [number, number] {
  return [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]];
}

export const isAtFit = (m: Matrix): boolean => scaleOf(m) <= MIN_SCALE + FIT_EPSILON;

export function zoomAt(m: Matrix, pointX: number, pointY: number, factor: number): Matrix {
  const current = scaleOf(m);
  const next = Math.min(MAX_SCALE, Math.max(MIN_SCALE, current * factor));

  if (next <= MIN_SCALE + FIT_EPSILON) {
    return IDENTITY;
  }
  return multiply(scaleAbout(pointX, pointY, next / current), m);
}

export const panBy = (m: Matrix, dx: number, dy: number): Matrix =>
  multiply(translate(dx, dy), m);

export function clampPan(m: Matrix): Matrix {
  const limit = PAN_LIMIT * Math.max(1, scaleOf(m));
  const x = Math.min(limit, Math.max(-limit, m[4]));
  const y = Math.min(limit, Math.max(-limit, m[5]));
  return [m[0], m[1], m[2], m[3], x, y];
}
