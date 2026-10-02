import { useTranslation } from 'react-i18next'
import { motion, useReducedMotion } from 'framer-motion'
import { Check } from 'lucide-react'
import { useEffect } from 'react'
import { useAppStore } from '../../stores/appStore'
import { spring } from '../../lib/animations'

export function CapsuleComplete() {
  const { t } = useTranslation()
  const reduced = useReducedMotion()
  const resetRecording = useAppStore((s) => s.resetRecording)
  const setPipelineState = useAppStore((s) => s.setPipelineState)

  useEffect(() => {
    const timer = setTimeout(() => {
      resetRecording()
      setPipelineState('idle')
    }, 1200)
    return () => clearTimeout(timer)
  }, [resetRecording, setPipelineState])

  return (
    <motion.div className="relative z-10 flex items-center justify-center gap-1.5 h-9 px-3">
      <motion.div
        initial={reduced ? false : { scale: 0, opacity: 0 }}
        animate={{ scale: 1, opacity: 1 }}
        transition={spring.smooth}
      >
        <Check size={14} className="text-white" />
      </motion.div>
      <span className="min-w-0 truncate text-[11px] text-white font-medium">
        {t('capsule.done')}
      </span>
    </motion.div>
  )
}
