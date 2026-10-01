import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { emit } from '@tauri-apps/api/event'
import { removeCorrectionRule } from '../../lib/tauri'
import { useAppStore } from '../../stores/appStore'
import type { LearnedCorrection } from '../../stores/appStore'

export const LEARNED_NOTICE_MS = 5000
export const UNDONE_NOTICE_MS = 1500

/** Learned corrections are shown one at a time, each with its own undo. */
export function CapsuleLearned() {
  const current = useAppStore((s) => s.learnedCorrections[0])
  if (!current) return null
  // Keyed so every correction starts with a fresh timer and undo state.
  return <LearnedNotice key={current.id} correction={current} />
}

function LearnedNotice({ correction }: { correction: LearnedCorrection }) {
  const { t } = useTranslation()
  const [busy, setBusy] = useState(false)
  const [undone, setUndone] = useState(false)

  useEffect(() => {
    const timer = setTimeout(
      () => {
        const { learnedCorrections, setLearnedCorrections } = useAppStore.getState()
        setLearnedCorrections(learnedCorrections.filter((c) => c.id !== correction.id))
      },
      undone ? UNDONE_NOTICE_MS : LEARNED_NOTICE_MS,
    )
    return () => clearTimeout(timer)
  }, [correction.id, undone])

  const handleUndo = async (e: React.MouseEvent) => {
    e.stopPropagation()
    setBusy(true)
    try {
      await removeCorrectionRule(correction.id)
      await emit('dictionary:changed')
      setUndone(true)
    } catch (err) {
      console.error('Failed to undo learned correction:', err)
      setBusy(false)
    }
  }

  const stopPointerPropagation = (e: React.PointerEvent) => {
    e.stopPropagation()
  }

  if (undone) {
    return (
      <div className="relative z-10 flex items-center justify-center h-9 px-3.5">
        <p className="text-[12px] text-white/90">{t('capsule.undoneLearnedCorrection')}</p>
      </div>
    )
  }

  return (
    <div
      className="relative z-10 flex items-center gap-2 h-9 pl-3.5 pr-1"
      title={t('capsule.learnedCorrection')}
    >
      <p className="flex-1 min-w-0 truncate text-[12px] text-white/90">
        <span className="text-white/55">{correction.pattern}</span>
        <span className="text-white/40"> → </span>
        {correction.replacement}
      </p>
      <button
        onPointerDown={stopPointerPropagation}
        onPointerUp={stopPointerPropagation}
        onClick={(e) => void handleUndo(e)}
        disabled={busy}
        className="shrink-0 h-7 px-2.5 rounded-[8px] text-[12px] text-white bg-white/15 hover:bg-white/25 border-none cursor-pointer disabled:opacity-50 focus-visible:outline-2 focus-visible:outline-white"
      >
        {t('capsule.undoLearnedCorrection')}
      </button>
    </div>
  )
}
