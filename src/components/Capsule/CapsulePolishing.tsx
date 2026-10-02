import { useTranslation } from 'react-i18next'
import { motion } from 'framer-motion'
import { X } from 'lucide-react'
import { abortRecording } from '../../lib/tauri'
import { CapsuleWorkIndicator } from './CapsuleWorkIndicator'

export function CapsulePolishing() {
  const { t } = useTranslation()

  const handleCancel = async (e: React.MouseEvent) => {
    e.stopPropagation()
    try {
      await abortRecording()
    } catch (err) {
      console.error('Failed to abort polishing:', err)
    }
  }

  return (
    <motion.div className="relative z-10 flex items-center gap-2 h-9 pl-3 pr-1">
      <CapsuleWorkIndicator tone="thinking" />
      <p className="text-[11px] text-white leading-snug truncate flex-1 min-w-0">
        {t('capsule.thinking')}
      </p>
      <button
        onPointerDown={(e) => e.stopPropagation()}
        onPointerUp={(e) => e.stopPropagation()}
        onClick={handleCancel}
        aria-label={t('capsule.cancelPolishing')}
        className="capsule-cancel"
      >
        <X size={12} />
      </button>
    </motion.div>
  )
}
