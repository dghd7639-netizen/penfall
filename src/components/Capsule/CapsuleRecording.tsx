import { useTranslation } from 'react-i18next'
import { useRef, useState } from 'react'
import { motion } from 'framer-motion'
import { Check, X } from 'lucide-react'
import { abortRecording } from '../../lib/tauri'
import { useRecording } from '../../hooks/useRecording'
import { useAppStore } from '../../stores/appStore'
import { Waveform } from './Waveform'
import { TranslateTargetChip } from './TranslateTargetChip'
import { DurationTimer } from './DurationTimer'

export function CapsuleRecording() {
  const { t } = useTranslation()
  const { stopRecording } = useRecording()
  const pending = useRef(false)
  const [busy, setBusy] = useState(false)

  const handleAction = async (e: React.MouseEvent, action: () => Promise<void>) => {
    e.stopPropagation()
    if (pending.current) return
    pending.current = true
    setBusy(true)
    try {
      await action()
    } catch {
      useAppStore.getState().setPipelineError(t('capsule.errors.unknown'))
    } finally {
      pending.current = false
      setBusy(false)
    }
  }

  const stopPointerPropagation = (e: React.PointerEvent) => {
    e.stopPropagation()
  }

  return (
    <motion.div className="relative z-10 flex items-center gap-1 h-9 px-1">
      <button
        onPointerDown={stopPointerPropagation}
        onPointerUp={stopPointerPropagation}
        onClick={(e) => void handleAction(e, abortRecording)}
        disabled={busy}
        aria-label={t('capsule.cancelRecording')}
        title={t('capsule.cancelRecording')}
        className="flex items-center justify-center shrink-0 w-7 h-7 rounded-[8px] text-white/80 hover:text-white bg-white/15 hover:bg-white/25 border-none cursor-pointer disabled:opacity-50 focus-visible:outline-2 focus-visible:outline-white"
      >
        <X size={16} />
      </button>
      <DurationTimer />
      <div
        className="flex-1 min-w-0 flex justify-center cursor-grab active:cursor-grabbing"
        aria-hidden="true"
      >
        <Waveform />
      </div>
      <TranslateTargetChip />
      <button
        onPointerDown={stopPointerPropagation}
        onPointerUp={stopPointerPropagation}
        onClick={(e) => void handleAction(e, stopRecording)}
        disabled={busy}
        aria-label={t('capsule.finishRecording')}
        title={t('capsule.finishRecording')}
        className="flex items-center justify-center shrink-0 w-7 h-7 rounded-[8px] text-neutral-950 bg-white/90 hover:bg-white border-none cursor-pointer disabled:opacity-50 focus-visible:outline-2 focus-visible:outline-white"
      >
        <Check size={18} />
      </button>
    </motion.div>
  )
}
