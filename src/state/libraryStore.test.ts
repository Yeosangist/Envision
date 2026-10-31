import { describe, expect, it } from 'vitest';
import { ancestryOf, resolveFolderId, type FolderId } from './libraryStore';
import { CommandError, type Folder } from '../types';

function folder(
  id: number,
  relPath: string,
  parentId: number | null = null,
  coverUrl: string | null = null,
): Folder {
  const name = relPath === '' ? 'Library' : (relPath.split('/').pop() as string);
  return { id, parent_id: parentId, name, rel_path: relPath, image_count: 0, cover_url: coverUrl };
}

describe('resolveFolderId', () => {
  const tree = [
    folder(1, ''),
    folder(2, 'Trips', 1),
    folder(3, 'Trips/Iceland', 2),
    folder(4, 'Portraits', 1),
  ];

  it('resolves the library root to the folder row for it', () => {
    expect(resolveFolderId(null, tree)).toBe(1);
  });

  it('passes through a concrete folder id untouched', () => {
    expect(resolveFolderId(3, tree)).toBe(3);
    expect(resolveFolderId(4, [])).toBe(4);
  });

  it('fails loudly when the library has not been indexed yet', () => {
    expect(() => resolveFolderId(null, [])).toThrow(CommandError);
    expect(() => resolveFolderId(null, [])).toThrow(/has not been indexed/);
  });

  it('does not mistake a nested folder for the root', () => {
    const decoy = [folder(7, 'Library', 1), folder(8, 'Trips', 7)];
    expect(() => resolveFolderId(null, decoy)).toThrow(CommandError);
  });

  it('typechecks: null is the root sentinel, 0 is a real id', () => {
    const ids: FolderId[] = [null, 0, 1];
    expect(ids.map((id) => id === null)).toEqual([true, false, false]);
    expect(resolveFolderId(0, tree)).toBe(0);
  });
});

describe('ancestryOf', () => {
  const tree = [
    folder(1, ''),
    folder(2, 'Trips', 1),
    folder(3, 'Trips/Iceland', 2),
    folder(4, 'Portraits', 1),
  ];

  it('walks outermost first and ends at the requested folder', () => {
    const chain: string[] = ancestryOf(tree, 3).map((f) => f.rel_path);
    expect(chain).toEqual(['', 'Trips', 'Trips/Iceland']);
  });

  it('resolves the root when no folder is selected', () => {
    const chain: string[] = ancestryOf(tree, null).map((f) => f.rel_path);
    expect(chain).toEqual(['']);
  });

  it('stops at a missing parent instead of looping forever', () => {
    const orphan = [folder(9, 'Orphan', 404)];
    expect(ancestryOf(orphan, 9).map((f) => f.rel_path)).toEqual(['Orphan']);
  });

  it('returns nothing for an empty tree', () => {
    expect(ancestryOf([], 1)).toEqual([]);
  });
});