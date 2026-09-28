import { Fragment, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { emit } from '@tauri-apps/api/event'
import { removeCorrectionRule } from '../../lib/tauri'
import { useAppStore } from '../../stores/appStore'

export const LEARNED_NOTICE_MS = 5000

export function CapsuleLearned() {
  const { t } = useTranslation()
  const corrections = useAppStore((s) => s.learnedCorrections)
  const setLearnedCorrections = useAppStore((s) => s.setLearnedCorrections)
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    if (corrections.length === 0) return
    const timer = setTimeout(() => setLearnedCorrections([]), LEARNED_NOTICE_MS)
    return () => clearTimeout(timer)
  }, [corrections, setLearnedCorrections])

  if (corrections.length === 0) return null

  const handleUndo = async (e: React.MouseEvent) => {
    e.stopPropagation()
    setBusy(true)
    try {
      await Promise.all(corrections.map((c) => removeCorrectionRule(c.id)))
      await emit('dictionary:changed')
      setLearnedCorrections([])
    } catch (err) {
      console.error('Failed to undo learned corrections:', err)
      setBusy(false)
    }
  }

  const stopPointerPropagation = (e: React.PointerEvent) => {
    e.stopPropagation()
  }

  return (
    <div
      className="relative z-10 flex items-center gap-2 h-9 pl-3.5 pr-1"
      title={t('capsule.learnedCorrection')}
    >
      <p className="flex-1 min-w-0 truncate text-[12px] text-white/90">
        {corrections.map((c, i) => (
          <Fragment key={c.id}>
            {i > 0 && <span className="text-white/40">，</span>}
            <span className="text-white/55">{c.pattern}</span>
            <span className="text-white/40"> → </span>
            {c.replacement}
          </Fragment>
        ))}
      </p>
      <button
        onPointerDown={stopPointerPropagation}
        onPointerUp={stopPointerPropagation}
        onClick={(e) => void handleUndo(e)}
        disabled={busy}
        className="shrink-0 h-7 px-2.5 rounded-full text-[12px] text-white bg-white/15 hover:bg-white/25 border-none cursor-pointer disabled:opacity-50 focus-visible:outline-2 focus-visible:outline-white"
      >
        {t('capsule.undoLearnedCorrection')}
      </button>
    </div>
  )
}
