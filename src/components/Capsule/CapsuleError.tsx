import { useEffect } from 'react'
import { motion, useReducedMotion } from 'framer-motion'
import { AlertCircle } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '../../stores/appStore'

export function CapsuleError() {
  const { t } = useTranslation()
  const reduced = useReducedMotion()
  const pipelineError = useAppStore((s) => s.pipelineError)
  const setPipelineError = useAppStore((s) => s.setPipelineError)
  const resetRecording = useAppStore((s) => s.resetRecording)

  useEffect(() => {
    const timer = setTimeout(() => {
      setPipelineError(null)
      // Only reset recording state if the pipeline is actually idle.
      // If the user started a new recording during the 2.5s error window,
      // don't overwrite the active pipeline state.
      const currentState = useAppStore.getState().pipelineState
      if (currentState === 'idle') {
        resetRecording()
      }
    }, 2500)
    return () => clearTimeout(timer)
  }, [setPipelineError, resetRecording, pipelineError])

  return (
    <motion.div
      className="relative z-10 flex items-center gap-2 h-9 px-3"
      initial={reduced ? false : { opacity: 0, x: -4 }}
      animate={{ opacity: 1, x: 0 }}
      transition={{ duration: 0.3, ease: 'easeOut' }}
    >
      <AlertCircle size={14} className="shrink-0 text-error" aria-hidden="true" />
      <p className="min-w-0 text-[11px] text-white truncate flex-1">
        {pipelineError || t('capsule.errors.unknown')}
      </p>
    </motion.div>
  )
}
