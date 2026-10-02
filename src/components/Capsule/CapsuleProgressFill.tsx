import { motion, useReducedMotion } from 'framer-motion'

// STT and polishing report no progress, so this is a time-based estimate that
// eases toward 90% and completes once output begins.
const ESTIMATE_SECONDS = 6
const ESTIMATE_CEILING = '90%'

export function CapsuleProgressFill({ done }: { done: boolean }) {
  const reduced = useReducedMotion()
  return (
    <motion.div
      aria-hidden="true"
      data-testid="capsule-progress"
      className="absolute inset-y-0 left-0 z-0 pointer-events-none bg-white/25"
      initial={reduced ? false : { width: '0%' }}
      animate={{ width: done ? '100%' : ESTIMATE_CEILING }}
      transition={
        reduced
          ? { duration: 0 }
          : done
            ? { duration: 0.2, ease: 'easeOut' }
            : { duration: ESTIMATE_SECONDS, ease: [0.15, 0.7, 0.3, 1] }
      }
    />
  )
}
