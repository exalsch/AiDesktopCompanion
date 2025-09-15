import { ref } from 'vue'

export function useWindowMode() {
  const winParam = new URLSearchParams(window.location.search).get('window')
  const isQuickActions = ref(winParam === 'quick-actions')
  const isCaptureOverlay = ref(winParam === 'capture-overlay')

  function addBodyClass() {
    try {
      if (isQuickActions.value) document.body.classList.add('qa-window')
    } catch {}
  }
  function removeBodyClass() {
    try {
      document.body.classList.remove('qa-window')
    } catch {}
  }

  return { isQuickActions, isCaptureOverlay, addBodyClass, removeBodyClass }
}
