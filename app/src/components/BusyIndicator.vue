<script setup lang="ts">
// Minimal busy indicator window for Quick Actions capture
</script>

<template>
  <div class="busy-root" role="status" aria-live="polite">
    <div class="spinner" />
    <div class="label">Working…</div>
  </div>
</template>

<style scoped>
.busy-root {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 12px;
  background: var(--adc-surface);
  color: var(--adc-fg);
  border: 1px solid var(--adc-border);
  border-radius: 10px;
  user-select: none;
}
.spinner {
  width: 16px;
  height: 16px;
  border: 2px solid var(--adc-border);
  border-top-color: var(--adc-accent);
  border-radius: 50%;
  animation: spin 1s linear infinite;
}
.label { font-size: 12px; color: var(--adc-fg-muted); }
@keyframes spin { from { transform: rotate(0); } to { transform: rotate(360deg); } }
</style>
