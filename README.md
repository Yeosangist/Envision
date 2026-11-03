# Envision

![GitHub Downloads (all assets, all releases)](https://img.shields.io/github/downloads/Yeosangist/Envision/total?style=flat&color=0a0)

A local-first image viewer for desktop. Load folders of images you already have and see them in a responsive masonry grid you can rearrange freely.

![](Assets/example.png)

## Features

- **Libraries.** Point Envision at a folder of images. Nothing is copied, and Envision writes nothing inside it unless you explicitly move a file yourself.
- **Pinterest board downloads.** Import a board with gallery-dl into a chosen destination, keeping its sections in subfolders. Set a default destination or ask each time, and choose a browser profile for private boards. Downloads run in the background; when one finishes, choose whether to add its folder as a library or rescan an existing library that contains it. gallery-dl must be installed separately and available on `PATH`.
- **Masonry browsing.** A virtualised, responsive grid with per-folder ordering that persists across restarts.
- **Reorder by dragging.** Moving an image to a new position in the grid updates the index and touches no files. The order is yours, and it stays yours.
- **Folder management.** Drag an image onto a folder card to move the file, with collision-safe renaming. Create and rename real directories too.
- **Duplicate detection.** SHA-256 over complete file contents. Identical files are grouped and badged and left exactly where they are.
- **Filesystem monitoring.** Changes made outside Envision are detected and can be rescanned.
- **Broad format support.** Common formats are decoded for thumbnails and viewing. HEIC, AVIF, JPEG XL, SVG, and camera raw are indexed and shown as placeholders; they open with your desktop's associated app.
- **System, light, grey, OLED black, and custom** theme options, plus import defaults and thumbnail CPU tuning in the settings UI.

## What it deliberately does not do

- Copy or move existing library images unless you explicitly move them in Envision. Pinterest imports are a separate, explicit download into the destination you choose; they are not copied into Envision's app data.
- Automatically reorganise or "tidy" your folders. Folder and image changes happen only when you explicitly make them in Envision.
- Delete a duplicate for you.

If a duplicate is worth removing, that is your call, in your file manager. What Envision gives you is knowing which files are identical and where they are.

## Tech Stack

- **Desktop Runtime:** Tauri 2
- **Frontend:** React + TypeScript + Tailwind CSS
- **State:** Zustand
- **Drag & Drop:** dnd-kit
- **Backend:** Rust
- **Database:** SQLite (per-machine, in app data)
- **Image Processing:** Rust `image` crate
- **Hashing:** `sha2`

## Getting Started

### Prerequisites

- Node.js 18 or higher
- Rust and Cargo
- A C compiler (for building Tauri)
- `gallery-dl` on `PATH` to import Pinterest boards (optional for other Envision features)

### Development

```bash
npm install
npm run tauri dev
```

### Building for Production

```bash
npm run tauri build
```

### Tests

```bash
cargo test --manifest-path src-tauri/Cargo.toml   # backend
npm test                                         # frontend unit tests
npm run typecheck
```

## Project Structure

```
src/
  api/            typed invoke wrappers, event subscriptions, image URLs
  features/
    launcher/     library list, add, relink, forget
    library/      masonry grid, folder cards, duplicates view
      masonry/    pure layout packer (+ tests)
    viewer/       fullscreen lightbox
  components/     ancestry, icons, scan indicator
  state/          library and settings stores
  types.ts        mirrors of the Rust domain types

src-tauri/
  migrations/     schema for the app database and library indexes
  src/
    commands/     Tauri command and event boundary
    domain/       shared types
    library/      registry, scan, fingerprint, queries, moves, watcher
    database/     connection setup and migrations
    paths/        path validation
    thumbnails/   content-addressed thumbnail cache
    images/       media classification
    platform/     OS app-data locations and thumbnail URLs
```

## How it is put together

Envision doesn't do anything inside your image folder unless you specifically make a new sub-folder or move an image into a different folder. Its registry, index, ordering, thumbnails, and fingerprint data live in the OS application-data directory:

```
<app-data>/
  settings.db                    library registry and preferences
  libraries/<id>/
    index.db                     folders, assets, SHA-256, display order
    thumbnails/<version>/<ab>/<sha256>.jpg
```

Your images are read where they lie. Thumbnail URLs use Tauri's asset protocol in packaged builds and a same-origin Vite middleware during development. Full-resolution images are returned as bytes over IPC using an asset id; the webview never receives a filesystem path.

## Notes
Been working on this for a while. First time working with Rust, I chose it for the build size.<br>
Importing a library for the first time might take a couple of minutes as it generates thumbnails, depending on how good your CPU is. You can limit the CPU usage in the import settings.<br>
Technically inspired by [Cascade](https://github.com/Yeosangist/Cascade), in that I wanted a broader, more flexible masonry image manager rather than just a viewer.