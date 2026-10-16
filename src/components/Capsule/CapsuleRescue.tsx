import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { writeText } from '@tauri-apps/plugin-clipboard-manager'
import { useAppStore } from '../../stores/appStore'
import type { DictationRescue } from '../../stores/appStore'

export const RESCUE_COPIED_MS = 1200

function dismissRescue() {
  const { setRescue, setPipelineError } = useAppStore.getState()
  setRescue(null)
  // An error that arrived while the rescue was showing is stale by now.
  setPipelineError(null)
}

/**
 * Shown when a dictation did not make it into the target app: the text stays
 * here until it is copied or dismissed, so it is never lost.
 */
export function CapsuleRescue() {
  const rescue = useAppStore((s) => s.rescue)
  if (!rescue) return null
  // Keyed so a newer rescue starts with a fresh copy state.
  return <RescueNotice key={rescue.text} rescue={rescue} />
}

function RescueNotice({ rescue }: { rescue: DictationRescue }) {
  const { t } = useTranslation()
  const [copied, setCopied] = useState(false)
  const [copyFailed, setCopyFailed] = useState(false)

  useEffect(() => {
    if (!copied) return
    const timer = setTimeout(dismissRescue, RESCUE_COPIED_MS)
    return () => clearTimeout(timer)
  }, [copied])

  const handleCopy = async (e: React.MouseEvent) => {
    e.stopPropagation()
    try {
      await writeText(rescue.text)
      setCopied(true)
    } catch (err) {
      console.error('Failed to copy rescued dictation:', err)
      setCopyFailed(true)
    }
  }

  const handleDismiss = (e: React.MouseEvent) => {
    e.stopPropagation()
    dismissRescue()
  }

  const stopPointerPropagation = (e: React.PointerEvent) => {
    e.stopPropagation()
  }

  if (copied) {
    return (
      <div className="relative z-10 flex items-center justify-center h-9 px-3.5">
        <p className="text-[12px] text-white/90">{t('capsule.rescueCopied')}</p>
      </div>
    )
  }

  return (
    <div
      className="relative z-10 flex items-center gap-2 h-9 pl-3.5 pr-1"
      title={rescue.reason ?? t('capsule.rescueHint')}
    >
      <span className="w-2 h-2 rounded-full flex-shrink-0 bg-[var(--color-error)]" />
      <p className="flex-1 min-w-0 truncate text-[12px] text-white/90">
        {rescue.text.replace(/\s+/g, ' ')}
      </p>
      <button
        onPointerDown={stopPointerPropagation}
        onPointerUp={stopPointerPropagation}
        onClick={(e) => void handleCopy(e)}
        className="shrink-0 h-7 px-2.5 rounded-full text-[12px] text-white bg-white/15 hover:bg-white/25 border-none cursor-pointer focus-visible:outline-2 focus-visible:outline-white"
      >
        {copyFailed ? t('capsule.rescueCopyFailed') : t('capsule.rescueCopy')}
      </button>
      <button
        onPointerDown={stopPointerPropagation}
        onPointerUp={stopPointerPropagation}
        onClick={handleDismiss}
        aria-label={t('capsule.rescueDismiss')}
        className="shrink-0 w-7 h-7 rounded-full text-[14px] leading-none text-white/70 hover:text-white hover:bg-white/15 bg-transparent border-none cursor-pointer focus-visible:outline-2 focus-visible:outline-white"
      >
        ×
      </button>
    </div>
  )
}
