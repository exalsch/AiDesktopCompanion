import { reactive } from 'vue'

export type ToastKind = 'error' | 'success'

export function useToast() {
  const toast = reactive({
    visible: false,
    message: '',
    kind: 'error' as ToastKind,
    hideTimer: 0 as any,
  })

  function showToast(message: string, kind: ToastKind = 'error', ms = 3500) {
    toast.message = message
    toast.kind = kind
    toast.visible = true
    if (toast.hideTimer) clearTimeout(toast.hideTimer)
    toast.hideTimer = setTimeout(() => { toast.visible = false }, ms)
  }

  return { toast, showToast }
}
