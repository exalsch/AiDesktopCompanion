## Project Docs

- Development guide: `app/DEVELOPMENT.md`
- UI Styles spec: `specs/UI_Styles.md`

## UI Styles

- Dynamic per-style CSS loading is implemented in `app/src/App.vue` via Vite `?url` assets.
- Available styles: `sidebar` (default), `tabs`, and `light` (new light desktop theme).
- Switch style via Settings: UI Style.

## Prompt History

- The Prompt section has subviews: `Chat` and `History`.
- Access History under Prompt in both sidebar and top-tabs layouts.

## Tray Icon

- Tauri v2 system tray is implemented with close-to-tray behavior in `app/src-tauri/src/lib.rs`.
