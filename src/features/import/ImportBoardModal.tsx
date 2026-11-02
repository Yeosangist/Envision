import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { CommandError, type ImportValidation } from '../../types';
import { useSettingsStore } from '../../state/settingsStore';

interface ImportBoardModalProps {
  isOpen: boolean;
  onClose: () => void;
  onStarted: (boardPath: string) => void;
}

export function ImportBoardModal({ isOpen, onClose, onStarted }: ImportBoardModalProps) {
  const importFolder = useSettingsStore((state) => state.importFolder);
  const importAsk = useSettingsStore((state) => state.importAsk);
  const importBrowserProfile = useSettingsStore((state) => state.importBrowserProfile);
  const [url, setUrl] = useState('');
  const [parentPath, setParentPath] = useState('');
  const [validation, setValidation] = useState<ImportValidation | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [isStarting, setIsStarting] = useState(false);
  const needsDestinationChoice = importAsk || !importFolder;

  useEffect(() => {
    if (!isOpen) return;
    setUrl('');
    setParentPath(!importAsk && importFolder ? importFolder : '');
    setValidation(null);
    setError(null);
    setIsStarting(false);
  }, [isOpen, importAsk, importFolder]);

  const handleSelectFolder = async () => {
    const selected = await open({
      directory: true,
      multiple: false,
    });
    if (selected && typeof selected === 'string') {
      setParentPath(selected);
    }
  };

  const handleValidate = async () => {
    setError(null);
    setValidation(null);

    if (!url.trim()) {
      setError('Please enter a Pinterest board URL');
      return;
    }

    if (!parentPath.trim()) {
      setError('Please select a destination folder');
      return;
    }

    try {
      const result = await invoke<ImportValidation>('validate_import_board', {
        url: url.trim(),
        parentPath: parentPath.trim(),
        boardNameOverride: null,
      });
      setValidation(result);
    } catch (e) {
      setError(CommandError.from(e).message);
    }
  };

  const handleStartImport = async () => {
    if (!validation) return;

    setIsStarting(true);
    setError(null);

    try {
      await invoke('start_import_board', {
        url: url.trim(),
        parentPath: validation.parentPath,
        browserProfileStr: importBrowserProfile,
      });
      onStarted(validation.boardPath);
      onClose();
    } catch (e) {
      setError(CommandError.from(e).message);
    } finally {
      setIsStarting(false);
    }
  };

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-gray-900 rounded-lg p-6 w-full max-w-md border border-gray-700">
        <h2 className="text-xl font-semibold text-gray-100 mb-4">Import Pinterest Board</h2>

        <div className="mb-4">
          <label className="block text-sm text-gray-400 mb-2">Pinterest Board URL</label>
          <input
            type="text"
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            placeholder="https://pinterest.com/username/boardname/"
            className="w-full bg-gray-800 border border-gray-700 rounded px-3 py-2 text-gray-100 focus:outline-none focus:border-blue-500"
            disabled={isStarting}
          />
        </div>

        {needsDestinationChoice && (
          <div className="mb-4">
            <label className="block text-sm text-gray-400 mb-2">Destination Folder</label>
            <div className="flex gap-2">
              <input
                type="text"
                value={parentPath}
                onChange={(e) => setParentPath(e.target.value)}
                placeholder="/path/to/folder"
                className="flex-1 bg-gray-800 border border-gray-700 rounded px-3 py-2 text-gray-100 focus:outline-none focus:border-blue-500"
                disabled={isStarting}
              />
              <button
                onClick={handleSelectFolder}
                className="px-4 py-2 bg-gray-700 hover:bg-gray-600 text-gray-100 rounded border border-gray-600"
                disabled={isStarting}
              >
                Browse
              </button>
            </div>
          </div>
        )}

        {error && (
          <div className="mb-4 p-3 bg-red-900/50 border border-red-700 rounded text-red-200 text-sm">
            {error}
          </div>
        )}

        {validation && (
          <div className="mb-4 p-3 bg-gray-800 border border-gray-700 rounded">
            <p className="text-sm text-gray-300">
              <span className="text-gray-400">Board:</span> {validation.boardName}
            </p>
            <p className="text-sm text-gray-300">
              <span className="text-gray-400">Destination:</span> {validation.boardPath}
            </p>
            {validation.isReimport && (
              <p className="text-sm text-yellow-400 mt-1">Re-importing to existing folder</p>
            )}
          </div>
        )}

        <div className="flex justify-end gap-3">
          <button
            onClick={onClose}
            className="px-4 py-2 bg-gray-700 hover:bg-gray-600 text-gray-100 rounded border border-gray-600"
            disabled={isStarting}
          >
            Cancel
          </button>
          {!validation ? (
            <button
              onClick={handleValidate}
              className="px-4 py-2 bg-blue-600 hover:bg-blue-500 text-white rounded"
              disabled={isStarting}
            >
              Validate
            </button>
          ) : (
            <button
              onClick={handleStartImport}
              className="px-4 py-2 bg-green-600 hover:bg-green-500 text-white rounded"
              disabled={isStarting}
            >
              {isStarting ? 'Starting…' : 'Start Import'}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
