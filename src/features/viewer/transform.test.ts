import { describe, expect, it } from 'vitest';
import {
  IDENTITY,
  MAX_SCALE,
  MIN_SCALE,
  PAN_LIMIT,
  ZOOM_STEP,
  applyToPoint,
  clampPan,
  isAtFit,
  multiply,
  panBy,
  scaleAbout,
  scaleOf,
  translate,
  zoomAt,
} from './transform';

/** A panned, 2x-zoomed view, to exercise anchoring away from identity. */
const ZOOMED_VIEW = multiply(translate(-40, 25), scaleAbout(0, 0, 2));

describe('scaleAbout', () => {
  it('fixes the point it scales about', () => {
    const m = scaleAbout(312, 180, 2.5);
    const fixed = applyToPoint(m, 312, 180);
    expect(fixed[0]).toBeCloseTo(312, 6);
    expect(fixed[1]).toBeCloseTo(180, 6);
    expect(scaleOf(m)).toBeCloseTo(2.5, 6);
  });
});

describe('zoomAt', () => {
  it('changes the scale, not just the position', () => {
    // The whole bug in one assertion: this used to return a pure translation,
    // so the scale stayed at 1 and "zooming" only slid the image sideways.
    const zoomed = zoomAt(IDENTITY, 100, 50, ZOOM_STEP);
    expect(scaleOf(zoomed)).toBeCloseTo(ZOOM_STEP, 6);
  });

  it('keeps the pixel under the cursor fixed', () => {
    // The cursor sits over a screen point; the local point currently rendered
    // there has to stay rendered there, in both directions.
    const screen: [number, number] = [312, 180];
    const k = scaleOf(ZOOMED_VIEW);
    const local: [number, number] = [
      (screen[0] - ZOOMED_VIEW[4]) / k,
      (screen[1] - ZOOMED_VIEW[5]) / k,
    ];
    expect(applyToPoint(ZOOMED_VIEW, local[0], local[1])[0]).toBeCloseTo(screen[0], 6);

    for (const factor of [ZOOM_STEP, 1 / ZOOM_STEP, ZOOM_STEP * ZOOM_STEP]) {
      const after = applyToPoint(zoomAt(ZOOMED_VIEW, screen[0], screen[1], factor), local[0], local[1]);
      expect(after[0]).toBeCloseTo(screen[0], 5);
      expect(after[1]).toBeCloseTo(screen[1], 5);
    }
  });

  it('does not creep toward the top-left corner as steps accumulate', () => {
    const centre: [number, number] = [400, 300];
    let m = IDENTITY;
    for (let i = 0; i < 12; i += 1) m = zoomAt(m, centre[0], centre[1], ZOOM_STEP);

    const mapped = applyToPoint(m, centre[0], centre[1]);
    expect(mapped[0]).toBeCloseTo(centre[0], 5);
    expect(mapped[1]).toBeCloseTo(centre[1], 5);
    expect(scaleOf(m)).toBeCloseTo(ZOOM_STEP ** 12, 5);
  });

  it('clamps at the 8x ceiling and then stops moving', () => {
    let m = IDENTITY;
    for (let i = 0; i < 40; i += 1) m = zoomAt(m, 100, 100, ZOOM_STEP);
    expect(scaleOf(m)).toBeCloseTo(MAX_SCALE, 5);

    const held = zoomAt(m, 100, 100, ZOOM_STEP);
    expect(scaleOf(held)).toBeCloseTo(MAX_SCALE, 5);
    expect(held).toEqual(m);
  });

  it('snaps to identity when zooming out past fit, dropping any pan', () => {
    const back = zoomAt(ZOOMED_VIEW, 50, 50, 0.1);
    expect(back).toEqual(IDENTITY);
    expect(scaleOf(back)).toBeCloseTo(MIN_SCALE, 6);
  });
});

describe('panBy', () => {
  it('moves the image exactly as far as the cursor travelled', () => {
    // One-for-one at any zoom. Composing the delta on the wrong side scaled it
    // by the zoom factor, so a drag overshot when zoomed in.
    const before = applyToPoint(ZOOMED_VIEW, 123, 456);
    const after = applyToPoint(panBy(ZOOMED_VIEW, 30, -18), 123, 456);

    expect(after[0]).toBeCloseTo(before[0] + 30, 6);
    expect(after[1]).toBeCloseTo(before[1] - 18, 6);
    expect(scaleOf(panBy(ZOOMED_VIEW, 30, -18))).toBeCloseTo(scaleOf(ZOOMED_VIEW), 6);
  });
});

describe('clampPan', () => {
  it('allows panning at fit', () => {
    const panned = clampPan(panBy(IDENTITY, 120, -80));
    expect(panned[4]).toBeCloseTo(120, 5);
    expect(panned[5]).toBeCloseTo(-80, 5);
  });

  it('is a no-op while at fit and untouched', () => {
    expect(clampPan(IDENTITY)).toEqual(IDENTITY);
  });

  it('bounds the travel, and allows more of it when zoomed in', () => {
    const far = translate(PAN_LIMIT * 10, -PAN_LIMIT * 10);

    const atFit = clampPan(multiply(far, IDENTITY));
    expect(atFit[4]).toBeCloseTo(PAN_LIMIT, 5);
    expect(atFit[5]).toBeCloseTo(-PAN_LIMIT, 5);

    const zoomedAndDragged = clampPan(panBy(scaleAbout(0, 0, MAX_SCALE), PAN_LIMIT * 10, 0));
    expect(zoomedAndDragged[4]).toBeGreaterThan(PAN_LIMIT);
  });

  it('preserves the scale, never the translation alone', () => {
    const m = scaleAbout(0, 0, 2);
    const clamped = clampPan(panBy(m, PAN_LIMIT * 5, 0));

    expect(scaleOf(clamped)).toBeCloseTo(2, 6);
    expect([clamped[0], clamped[1], clamped[2], clamped[3]]).toEqual([m[0], m[1], m[2], m[3]]);
  });
});

describe('scaleOf / isAtFit', () => {
  it('reads a uniform scale back out of the matrix', () => {
    expect(scaleOf(IDENTITY)).toBeCloseTo(1, 6);
    expect(scaleOf(zoomAt(IDENTITY, 0, 0, 3))).toBeCloseTo(3, 6);
  });

  it('reports fit for identity and a panned view, but not a scaled one', () => {
    expect(isAtFit(IDENTITY)).toBe(true);
    expect(isAtFit(panBy(IDENTITY, 300, -120))).toBe(true);
    expect(isAtFit(zoomAt(IDENTITY, 0, 0, 1.01))).toBe(false);
  });
});
