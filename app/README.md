# Vue 3 + TypeScript + Vite

This template should help get you started developing with Vue 3 and TypeScript in Vite. The template uses Vue 3 `<script setup>` SFCs, check out the [script setup docs](https://v3.vuejs.org/api/sfc-script-setup.html#sfc-script-setup) to learn more.

Learn more about the recommended Project Setup and IDE Support in the [Vue Docs TypeScript Guide](https://vuejs.org/guide/typescript/overview.html#project-setup).

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
