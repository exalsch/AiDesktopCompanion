import { ref } from 'vue'

export function useWindowMode() {
  const winParam = new URLSearchParams(window.location.search).get('window')
  const isQuickActions = ref(winParam === 'quick-actions')
  const isCaptureOverlay = ref(winParam === 'capture-overlay')
  const isQuickActionsBusy = ref(winParam === 'qa-busy')

  function addBodyClass() {
    try {
      if (isQuickActions.value) document.body.classList.add('qa-window')
      if (isQuickActionsBusy.value) document.body.classList.add('qa-busy-window')
    } catch {}
  }
  function removeBodyClass() {
    try {
      document.body.classList.remove('qa-window')
      document.body.classList.remove('qa-busy-window')
    } catch {}
  }

  return { isQuickActions, isCaptureOverlay, isQuickActionsBusy, addBodyClass, removeBodyClass }
}
