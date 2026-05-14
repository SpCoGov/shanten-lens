# AGENTS.md

## Project Overview

shanten-lens is a desktop application with a Tauri + React + TypeScript frontend and a Python backend.

The backend uses a MITM approach to obtain information from Qingyunzhizhi. The frontend and backend communicate through WebSocket connections.

## Repository Structure

### Backend

All backend code is stored in `backend/`.

- `backend/autorun/runner.py` is responsible for automatically rerolling starts.
- `backend/autorun/util/` stores the tools required for automation, including play and tile-swap guidance.
- `backend/bot/` defines how game actions are performed by sending packets.
- `backend/config/` contains the backend configuration system, including the config registry and config manager.
- `backend/data/` stores the current game's amulet registry, seal registry, and registry loaders.
- `backend/mitm/` contains the mitmproxy scripts plus packet decoders and parsers.
- `backend/mitm/hooks.py` handles all packets from the client and server. Fuse logic, fuse handling, and game-state parsing are all implemented in this file.
- `backend/model/` defines game registry entries and game state models.
- `backend/run_server.py` is the backend entry point. It starts MITM, defines the packet bot, and starts the backend and frontend WebSocket servers.

### Frontend

All frontend code is stored in `app/`.

- `app/src/` contains the main frontend UI.
- `app/src-tauri/` contains the Tauri Rust backend.
- `app/public/` stores almost all asset files, including amulet icons, amulet backgrounds, seal icons, and tile images.
- `app/src/components/` stores frontend components, such as amulet components.
- `app/src/lib/` stores most frontend logic, including amulet price calculation, config storage, amulet effect registries, fuse-rule storage, update checks, toast handling, themes, message boxes, localization, and backend communication.
- `app/src/locales/` stores localization language files.
- `app/src/pages/` stores all frontend pages except the home page.
- `app/src/styles/` stores color schemes for all themes.
- `app/src/windows/` stores popup windows, such as the message box and settings window.
- `app/src/App.tsx` defines the main window and the home page.
- `app/src/main.tsx`, `app/src/msgbox.tsx`, and `app/src/settings_main.tsx` start the corresponding windows and initialize services such as theme and localization.

## Common Change Guidelines

1. For all frontend text changes, use i18n. Do not hard-code user-facing text inside components. Match the style of existing translations, including the existing translations for domain-specific terms.

2. For component colors, define colors in `app/src/styles/theme.css` unless there is a special reason not to. Colors must support the different themes, including light and dark themes. Components should reference theme colors instead of hard-coding color values.

3. When adding or modifying fuse items, update the logic in `backend/mitm/hooks.py`, inside `def on_outbound(view: Dict) -> Tuple[str, Any]`. If the change includes configuration items, also update `backend/config/registry.py` and `app/src/lib/fuseStore.ts`, then display the corresponding configuration item in `app/src/pages/FusePage.tsx`.

4. When changing shared behavior:
   - update backend logic
   - update frontend consumers
   - update related config entries
   - update localization if the change is user-facing

## Protocol Compatibility

IMPORTANT:

- WebSocket payload formats and packet parsing behavior must remain backward compatible unless the change is explicitly intended to break compatibility.

## Restrictions

- Do not refactor unrelated files.
- Do not rename public WebSocket payload fields without updating all consumers.
- Do not introduce large new dependencies unless necessary.
- Preserve existing project structure and coding style.
- Keep diffs minimal and focused.

## Running The Project

- Start the frontend with `pnpm run tauri:dev`.
- Start the backend with `python backend/run_server.py`.

Use `pnpm` only. Do not use `npm`.

## Before Finishing Changes

- Ensure TypeScript compiles successfully.
- Ensure Python code has no syntax errors.
