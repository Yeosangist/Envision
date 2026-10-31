
/** Packs masonry tiles while keeping stored order independent of visual position. */
/** Clamp height/width ratios to keep extreme portraits and panoramas usable; 0.2 supports panoramas up to 5:1. */
export const MIN_ASPECT = 0.2;
export const MAX_ASPECT = 2.6;

/** Assumed ratio when an image's dimensions are unknown. */
export const DEFAULT_ASPECT = 1;

export interface MasonryInput {
  id: number;
  /** **height / width.** A 4:1 panorama is `0.25`, not `4`. */
  aspectRatio: number;
}

export interface PlacedTile {
  /** Index in the logical array — this is the user's order, not the visual one. */
  index: number;
  id: number;
  x: number;
  y: number;
  width: number;
  height: number;
  column: number;
}

export interface MasonryLayout {
  tiles: PlacedTile[];
  /** Tile indices per column, each sorted top-to-bottom. Used for visibility queries. */
  columns: number[][];
  totalHeight: number;
  columnCount: number;
  columnWidth: number;
  /** True when the layout has no tiles, so a container needs no explicit height. */
  empty: boolean;
}

export interface MasonryOptions {
  width: number;
  /** Width a column would ideally have. Columns stretch to fill the remainder. */
  targetColumnWidth?: number;
  spacing?: number;
}

export const DEFAULT_TARGET_COLUMN_WIDTH = 200;
export const DEFAULT_SPACING = 8;

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

/** Pack into shortest columns, stretching them to fill the container width. */
export function packMasonry(
  items: readonly MasonryInput[],
  options: MasonryOptions,
): MasonryLayout {
  const { width } = options;
  const targetColumnWidth = options.targetColumnWidth ?? DEFAULT_TARGET_COLUMN_WIDTH;
  const spacing = options.spacing ?? DEFAULT_SPACING;

  if (items.length === 0 || width <= 0) {
    return {
      tiles: [],
      columns: [],
      totalHeight: 0,
      columnCount: 0,
      columnWidth: 0,
      empty: true,
    };
  }

  const columnCount = Math.max(
    1,
    Math.floor((width + spacing) / (targetColumnWidth + spacing)),
  );
  const columnWidth = (width - spacing * (columnCount - 1)) / columnCount;

  const columnBottoms = new Array<number>(columnCount).fill(0);
  const columns: number[][] = Array.from({ length: columnCount }, () => []);
  const tiles: PlacedTile[] = new Array(items.length);

  items.forEach((item, index) => {
    const raw = Number.isFinite(item.aspectRatio) && item.aspectRatio > 0 ? item.aspectRatio : DEFAULT_ASPECT;
    const aspect = clamp(raw, MIN_ASPECT, MAX_ASPECT);
    const height = Math.max(1, columnWidth * aspect);

    // Greedy shortest column: the tile that makes the top of the grid most even.
    let column = 0;
    for (let candidate = 1; candidate < columnCount; candidate += 1) {
      if (columnBottoms[candidate] < columnBottoms[column]) {
        column = candidate;
      }
    }

    const y = columnBottoms[column];
    tiles[index] = {
      index,
      id: item.id,
      x: column * (columnWidth + spacing),
      y,
      width: columnWidth,
      height,
      column,
    };
    columns[column].push(index);
    columnBottoms[column] = y + height + spacing;
  });

  const totalHeight = Math.max(...columnBottoms) - spacing;

  return {
    tiles,
    columns,
    totalHeight: Math.max(0, totalHeight),
    columnCount,
    columnWidth,
    empty: false,
  };
}

/** Return visible tile indices by binary-searching each column's vertical band. */
export function visibleIndices(
  layout: MasonryLayout,
  top: number,
  bottom: number,
): number[] {
  if (layout.empty) return [];

  const found = new Set<number>();

  for (const column of layout.columns) {
    let low = 0;
    let high = column.length;

    while (low < high) {
      const middle = (low + high) >> 1;
      const tile = layout.tiles[column[middle]];
      if (tile.y + tile.height < top) {
        low = middle + 1;
      } else {
        high = middle;
      }
    }

    for (let cursor = low; cursor < column.length; cursor += 1) {
      const index = column[cursor];
      if (layout.tiles[index].y > bottom) break;
      found.add(index);
    }
  }

  return [...found].sort((a, b) => a - b);
}

/** Move the item at `from` to `to`, returning a new array. */
export function moveItem<T>(items: readonly T[], from: number, to: number): T[] {
  const next = items.slice();
  if (from < 0 || from >= next.length) return next;
  const target = clamp(to, 0, next.length - 1);
  const [moved] = next.splice(from, 1);
  next.splice(target, 0, moved);
  return next;
}