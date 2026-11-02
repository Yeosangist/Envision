import { readFileSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import path from 'node:path';
import { defineConfig, type Plugin } from 'vite';
import process from 'node:process';

const host = process.env.TAURI_DEV_HOST;

const packageVersion = JSON.parse(
  readFileSync('package.json', 'utf8'),
) as { version: string };

const THUMB_PREFIX = '/__thumbs';

function tauriConfig(): { identifier?: string } {
  try {
    return JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8')) as {
      identifier?: string;
    };
  } catch {
    return {};
  }
}

function appDataDir(identifier: string): string | null {
  if (process.platform === 'linux') {
    const base = process.env.XDG_DATA_HOME || path.join(homedir(), '.local', 'share');
    return path.join(base, identifier);
  }
  if (process.platform === 'darwin') {
    return path.join(homedir(), 'Library', 'Application Support', identifier);
  }
  return null;
}

function safeResolve(root: string, requestPath: string): string | null {
  const segments = requestPath.split('/').filter((segment) => segment.length > 0);
  if (segments.length === 0) return null;

  const resolved = path.resolve(root, ...segments);
  const base = path.resolve(root);
  if (resolved !== base && !resolved.startsWith(base + path.sep)) return null;
  return resolved;
}

function devThumbnails(): Plugin {
  const identifier = tauriConfig().identifier;
  const dataDir = identifier ? appDataDir(identifier) : null;

  return {
    name: 'envision-dev-thumbnails',
    // Never part of a production bundle.
    apply: 'serve',
    configureServer(server) {
      if (!dataDir) {
        server.config.logger.warn(
          `[envision] thumbnail middleware disabled: no app-data directory known for ${process.platform}`,
        );
        return;
      }
      server.config.logger.info(`[envision] thumbnails served from ${dataDir}`);

      server.middlewares.use(THUMB_PREFIX, async (request, response) => {
        try {
          const url = new URL(request.url ?? '/', 'http://localhost');
          const relative = decodeURIComponent(url.pathname);

          // `mount` strips the prefix, but be defensive rather than assume it.
          const wanted = relative.startsWith(THUMB_PREFIX)
            ? relative.slice(THUMB_PREFIX.length)
            : relative;

          const [libraryId, ...rest] = wanted.split('/').filter(Boolean);
          if (!libraryId || rest.length === 0) {
            response.statusCode = 400;
            response.end('expected /__thumbs/<library-id>/<file>');
            return;
          }

          const file = safeResolve(
            path.join(dataDir, 'libraries', libraryId, 'thumbnails'),
            rest.join('/'),
          );
          if (!file) {
            response.statusCode = 403;
            response.end('refused');
            return;
          }

          const body = await readFile(file);
          response.setHeader('Content-Type', 'image/jpeg');
          // Long enough that scrolling back over a folder costs nothing.
          response.setHeader('Cache-Control', 'private, max-age=3600');
          response.end(body);
        } catch {
          // Missing thumbnail: 404 so the tile falls back to its placeholder
          // rather than hanging, and so the miss is visible in the network tab.
          response.statusCode = 404;
          response.end('no thumbnail');
        }
      });
    },
  };
}

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [devThumbnails()],

  define: {
    'import.meta.env.PACKAGE_VERSION': JSON.stringify(packageVersion.version),
  },

  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: 'ws',
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));