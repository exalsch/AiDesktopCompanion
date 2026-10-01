import { reactive, computed } from 'vue'
import type { Ref } from 'vue'
import { useSttFileJob } from './useSttFileJob'

export function useBusy(modelsLoading: Ref<boolean>) {
  const busy = reactive({ prompt: false, tts: false, stt: false })
  // A file job outlives its panel, so its state is read straight from the job
  // rather than reported by the component.
  const fileJob = useSttFileJob()
  const isBusy = computed(() => busy.prompt || busy.tts || busy.stt || fileJob.state.running || !!modelsLoading.value)
  return { busy, isBusy }
}
