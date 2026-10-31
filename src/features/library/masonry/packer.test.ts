import { describe, expect, it } from 'vitest';
import {
  DEFAULT_ASPECT,
  MAX_ASPECT,
  MIN_ASPECT,
  moveItem,
  packMasonry,
  visibleIndices,
  type MasonryInput,
} from './packer';

function items(count: number, aspect = 1): MasonryInput[] {
  return Array.from({ length: count }, (_, index) => ({ id: index + 1, aspectRatio: aspect }));
}

describe('packMasonry', () => {
  it('fills the container width exactly, with no ragged gutter', () => {
    const spacing = 10;
    const layout = packMasonry(items(30), { width: 660, targetColumnWidth: 200, spacing });

    expect(layout.columnCount).toBe(3);
    expect(layout.columnWidth).toBeCloseTo((660 - spacing * 2) / 3, 5);

    const rightmost = Math.max(...layout.tiles.map((tile) => tile.x + tile.width));
    expect(rightmost).toBeCloseTo(660, 5);
  });

  it('derives column count from the target width and spacing', () => {
    const cases: Array<[number, number]> = [
      [300, 1],
      [410, 2],
      [660, 3],
      [1000, 4],
      [1600, 7],
    ];
    for (const [width, expected] of cases) {
      const layout = packMasonry(items(10), { width, targetColumnWidth: 200, spacing: 10 });
      expect(layout.columnCount, `width ${width}`).toBe(expected);
    }
  });

  it('never drops below one column on a narrow container', () => {
    const layout = packMasonry(items(5), { width: 120, targetColumnWidth: 200, spacing: 10 });
    expect(layout.columnCount).toBe(1);
    expect(layout.columnWidth).toBeCloseTo(120, 5);
  });

  it('clamps extreme aspect ratios', () => {
    const layout = packMasonry(
      [
        { id: 1, aspectRatio: 0.01 },
        { id: 2, aspectRatio: 99 },
      ],
      { width: 660, targetColumnWidth: 200, spacing: 10 },
    );

    expect(layout.tiles[0].height).toBeCloseTo(layout.columnWidth * MIN_ASPECT, 5);
    expect(layout.tiles[1].height).toBeCloseTo(layout.columnWidth * MAX_ASPECT, 5);
  });

  it('reads aspectRatio as height over width', () => {
    const columnWidth = 210;
    const layout = packMasonry(
      [
        { id: 1, aspectRatio: 1 / 4 }, // 4:1 panorama
        { id: 2, aspectRatio: 2 }, // 1:2 portrait
        { id: 3, aspectRatio: 1 }, // square
      ],
      { width: columnWidth, targetColumnWidth: columnWidth, spacing: 10 },
    );

    expect(layout.columnWidth).toBeCloseTo(columnWidth, 5);
    expect(layout.tiles[0].height).toBeCloseTo(columnWidth / 4, 3);
    expect(layout.tiles[1].height).toBeCloseTo(columnWidth * 2, 3);
    expect(layout.tiles[2].height).toBeCloseTo(columnWidth, 3);
  });

  it('gives ordinary images a tile that matches their real proportions', () => {
    const images: Array<[number, number]> = [
      [4000, 1000], // 4:1 panorama
      [6000, 2000], // 3:1
      [3000, 1000], // 3:1
      [4032, 3024], // 4:3
      [1600, 900], // 16:9
      [1000, 2000], // 1:2 portrait
      [1000, 1000], // square
    ];

    for (const [width, height] of images) {
      const layout = packMasonry(
        [{ id: 1, aspectRatio: height / width }],
        { width: 210, targetColumnWidth: 210, spacing: 10 },
      );
      const expected = (210 * height) / width;

      expect(
        layout.tiles[0].height,
        `${width}x${height} should tile at ${expected.toFixed(1)}px tall`,
      ).toBeCloseTo(expected, 3);
    }
  });

  it('bounds tiles for images beyond the clamp', () => {
    const layout = packMasonry(
      [
        { id: 1, aspectRatio: 1 / 40 }, // 40:1 equirectangular
        { id: 2, aspectRatio: 40 }, // 1:40 pillar
      ],
      { width: 210, targetColumnWidth: 210, spacing: 10 },
    );

    expect(layout.tiles[0].height).toBeCloseTo(210 * MIN_ASPECT, 3);
    expect(layout.tiles[1].height).toBeCloseTo(210 * MAX_ASPECT, 3);
  });

  it('falls back to a square for missing or nonsense dimensions', () => {
    const layout = packMasonry(
      [
        { id: 1, aspectRatio: 0 },
        { id: 2, aspectRatio: Number.NaN },
        { id: 3, aspectRatio: -5 },
      ],
      { width: 400, targetColumnWidth: 200, spacing: 10 },
    );

    for (const tile of layout.tiles) {
      expect(tile.height).toBeCloseTo(layout.columnWidth * DEFAULT_ASPECT, 5);
    }
  });

  it('always places a tile in the column that was shortest', () => {
    // 420px / (200+10) = 2.004 -> two columns, width 205.
    const layout = packMasonry(
      [
        { id: 1, aspectRatio: 3 },
        { id: 2, aspectRatio: 1 },
        { id: 3, aspectRatio: 1 },
        { id: 4, aspectRatio: 0.5 },
      ],
      { width: 420, targetColumnWidth: 200, spacing: 10 },
    );
    expect(layout.columnCount).toBe(2);

    // Replay the packing and confirm each placement matched the shortest column.
    const bottoms = new Array(layout.columnCount).fill(0);
    for (const tile of layout.tiles) {
      const shortest = bottoms.indexOf(Math.min(...bottoms));
      expect(tile.column, `tile ${tile.id}`).toBe(shortest);
      expect(tile.y).toBe(bottoms[tile.column]);
      bottoms[tile.column] = tile.y + tile.height + 10;
    }
  });

  it('preserves reading order while scattering tiles across columns', () => {
    const aspects = [2.4, 0.6, 0.6, 2.4, 0.6, 0.6, 2.4, 0.6, 0.6];
    const input = aspects.map((aspectRatio, index) => ({ id: index + 1, aspectRatio }));
    const layout = packMasonry(input, { width: 800, targetColumnWidth: 200, spacing: 10 });

    // Every tile knows its own logical position.
    for (const tile of layout.tiles) {
      expect(tile.index).toBe(tile.id - 1);
    }

    for (let i = 1; i < layout.tiles.length; i += 1) {
      expect(layout.tiles[i].y).toBeGreaterThanOrEqual(layout.tiles[i - 1].y);
    }

    // But position is genuinely a layout result, not a column-major index: the
    // tiles are scattered across several columns with differing heights.
    expect(new Set(layout.tiles.map((tile) => tile.column)).size).toBeGreaterThan(1);
    expect(new Set(layout.tiles.map((tile) => Math.round(tile.y))).size).toBeGreaterThan(3);
  });

  it('reordering the input changes the rendered geometry', () => {
    // This is the whole point: a reorder is a change to the ordered array, and
    // the layout follows from it. Nothing about the pixels implies the order.
    const aspects = [2.4, 0.6, 0.6, 2.4, 0.6, 0.6, 2.4, 0.6, 0.6];
    const items = aspects.map((aspectRatio, index) => ({ id: index + 1, aspectRatio }));

    const before = packMasonry(items, { width: 800, targetColumnWidth: 200, spacing: 10 });
    const swapped = [items[4], items[1], items[2], items[3], items[0], items[5], items[6], items[7], items[8]];
    const after = packMasonry(swapped, { width: 800, targetColumnWidth: 200, spacing: 10 });

    // Item 1 kept id 1 but changed size, because it now sits against a different
    // column height. Same ids, different geometry: the order drives the layout.
    const positionOf = (layout: typeof before, id: number) => {
      const tile = layout.tiles.find((candidate) => candidate.id === id)!;
      return { x: tile.x, y: tile.y, height: tile.height };
    };

    expect(positionOf(before, 1)).not.toEqual(positionOf(after, 1));
    // And the grid still spans its container either way.
    expect(Math.max(...after.tiles.map((tile) => tile.x + tile.width))).toBeCloseTo(800, 5);
  });

  it('reports a total height that contains every tile', () => {
    const layout = packMasonry(items(50, 0.8), { width: 800, targetColumnWidth: 200, spacing: 10 });

    const lowest = Math.max(...layout.tiles.map((tile) => tile.y + tile.height));
    expect(layout.totalHeight).toBeGreaterThanOrEqual(lowest);
  });

  it('is empty for an empty list or a collapsed container', () => {
    expect(packMasonry([], { width: 800 }).empty).toBe(true);
    expect(packMasonry(items(5), { width: 0 }).empty).toBe(true);
    expect(packMasonry(items(5), { width: 800 }).empty).toBe(false);
  });

  it('is stable: packing the same input twice gives identical geometry', () => {
    const input = items(37, 1.4);
    const a = packMasonry(input, { width: 1000, targetColumnWidth: 200, spacing: 8 });
    const b = packMasonry(input, { width: 1000, targetColumnWidth: 200, spacing: 8 });
    expect(a.tiles).toEqual(b.tiles);
  });
});

describe('visibleIndices', () => {
  const layout = packMasonry(items(200, 1), { width: 800, targetColumnWidth: 200, spacing: 10 });

  it('returns only tiles inside the band', () => {
    const top = 500;
    const bottom = 900;
    const visible = new Set(visibleIndices(layout, top, bottom));

    for (const tile of layout.tiles) {
      const intersects = tile.y + tile.height >= top && tile.y <= bottom;
      expect(visible.has(tile.index)).toBe(intersects);
    }
  });

  it('catches every column for a band spanning several of them', () => {
    const visible = visibleIndices(layout, 0, layout.totalHeight);
    expect(visible.length).toBe(layout.tiles.length);
  });

  it('returns indices in logical order regardless of column', () => {
    const visible = visibleIndices(layout, 200, 1400);
    const sorted = [...visible].sort((a, b) => a - b);
    expect(visible).toEqual(sorted);
  });

  it('handles a band that starts mid-tile', () => {
    const tile = layout.tiles[0];
    const visible = visibleIndices(layout, tile.y + tile.height - 1, tile.y + tile.height);
    expect(visible).toContain(0);
  });

  it('is empty for an empty layout', () => {
    expect(visibleIndices(packMasonry([], { width: 800 }), 0, 500)).toEqual([]);
  });
});

describe('moveItem', () => {
  it('moves an item and leaves the rest in order', () => {
    expect(moveItem(['a', 'b', 'c', 'd'], 0, 2)).toEqual(['b', 'c', 'a', 'd']);
    expect(moveItem(['a', 'b', 'c', 'd'], 3, 0)).toEqual(['d', 'a', 'b', 'c']);
    expect(moveItem(['a', 'b', 'c', 'd'], 1, 3)).toEqual(['a', 'c', 'd', 'b']);
  });

  it('clamps out-of-range targets and ignores invalid sources', () => {
    expect(moveItem(['a', 'b', 'c'], 0, 99)).toEqual(['b', 'c', 'a']);
    expect(moveItem(['a', 'b', 'c'], -1, 0)).toEqual(['a', 'b', 'c']);
    expect(moveItem(['a', 'b', 'c'], 99, 0)).toEqual(['a', 'b', 'c']);
  });

  it('does not mutate the input', () => {
    const original = ['a', 'b', 'c'];
    moveItem(original, 0, 2);
    expect(original).toEqual(['a', 'b', 'c']);
  });
});