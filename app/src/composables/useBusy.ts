import { reactive } from 'vue'
import type { Ref } from 'vue'

export function useBusy(modelsLoading: Ref<boolean>) {
  const busy = reactive({ prompt: false, tts: false, stt: false })
  const isBusy = () => busy.prompt || busy.tts || busy.stt || !!modelsLoading.value
  return { busy, isBusy }
}
