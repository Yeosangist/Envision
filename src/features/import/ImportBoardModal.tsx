import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { CommandError, type ImportValidation } from '../../types';
import { useSettingsStore } from '../../state/settingsStore';
import { CloseIcon } from '../../components/icons';

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

  useEffect(() => {
    if (!isOpen) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !isStarting) onClose();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [isOpen, isStarting, onClose]);

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
    <>
      <button
        type="button"
        className="modal-scrim"
        aria-label="Cancel import"
        onClick={() => !isStarting && onClose()}
      />
      <section
        className="modal new-folder-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="import-board-title"
      >
        <header className="modal-heading">
          <h2 id="import-board-title">Import Pinterest Board</h2>
          <button
            type="button"
            className="icon-button"
            onClick={onClose}
            disabled={isStarting}
            aria-label="Close"
            title="Close"
          >
            <CloseIcon />
          </button>
        </header>

        <div className="modal-body import-board-body">
          <div className="import-board-field">
            <label htmlFor="import-board-url">Pinterest Board URL</label>
            <input
              id="import-board-url"
              type="text"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              placeholder="https://pinterest.com/username/boardname/"
              disabled={isStarting}
            />
          </div>

          {needsDestinationChoice && (
            <div className="import-board-field">
              <label htmlFor="import-board-destination">Destination Folder</label>
              <div className="import-board-row">
                <input
                  id="import-board-destination"
                  type="text"
                  value={parentPath}
                  onChange={(e) => setParentPath(e.target.value)}
                  placeholder="/path/to/folder"
                  disabled={isStarting}
                />
                <button type="button" onClick={handleSelectFolder} disabled={isStarting}>
                  Browse
                </button>
              </div>
            </div>
          )}

          {error && (
            <div className="import-board-error" role="alert">
              {error}
            </div>
          )}

          {validation && (
            <div className="import-board-validation">
              <p>
                <span className="import-board-key">Board:</span> {validation.boardName}
              </p>
              <p>
                <span className="import-board-key">Destination:</span> {validation.boardPath}
              </p>
              {validation.isReimport && (
                <p className="import-board-reimport">Re-importing to existing folder</p>
              )}
            </div>
          )}
        </div>

        <footer className="new-folder-actions">
          <button type="button" onClick={onClose} disabled={isStarting}>
            Cancel
          </button>
          {!validation ? (
            <button type="button" className="primary" onClick={handleValidate} disabled={isStarting}>
              Validate
            </button>
          ) : (
            <button type="button" className="primary" onClick={handleStartImport} disabled={isStarting}>
              {isStarting ? 'Starting…' : 'Start Import'}
            </button>
          )}
        </footer>
      </section>
    </>
  );
}
