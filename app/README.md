## Project Docs

- Development guide: `app/DEVELOPMENT.md`
- UI Styles spec: `specs/UI_Styles.md`

## UI Styles

- Dynamic per-style CSS loading is implemented in `app/src/App.vue` via Vite `?url` assets.
- Available styles: `sidebar-dark` (default) and `sidebar-light`.
- Switch style via Settings: UI Style.

## Prompt History

- The Prompt section has subviews: `Chat` and `History`.
- Access History under Prompt in the sidebar layout.

## Tray Icon

- Tauri v2 system tray is implemented with close-to-tray behavior in `app/src-tauri/src/lib.rs`.
