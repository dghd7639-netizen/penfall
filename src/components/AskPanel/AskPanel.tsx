import { useCallback, useEffect, useRef, useState } from 'react'
import { Loader2, Mic, Square } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import {
  abortAskDictation,
  startAskDictation,
  stopAskDictation,
  takePendingAskMessage,
} from '../../lib/tauri'

interface AskPanelProps {
  embedded?: boolean
  showHeader?: boolean
  title?: string
}

interface AskResultPayload {
  question: string
  answer: string
}

export function AskPanel({ embedded = false, showHeader = true, title = 'Ask' }: AskPanelProps) {
  const { t } = useTranslation()
  const [answer, setAnswer] = useState('')
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)
  const [dictationState, setDictationState] = useState<'idle' | 'recording' | 'processing'>('idle')
  const loadingRef = useRef(loading)
  const dictationStateRef = useRef(dictationState)

  useEffect(() => {
    loadingRef.current = loading
    dictationStateRef.current = dictationState
  }, [dictationState, loading])

  const setBusy = useCallback((next: boolean) => {
    loadingRef.current = next
    setLoading(next)
  }, [])

  const setAskDictationState = useCallback((next: 'idle' | 'recording' | 'processing') => {
    dictationStateRef.current = next
    setDictationState(next)
  }, [])

  const applyResult = useCallback(
    (payload: AskResultPayload) => {
      setAnswer(payload.answer)
      setError('')
      setAskDictationState('idle')
      setBusy(false)
    },
    [setAskDictationState, setBusy],
  )

  const applyError = useCallback(
    (message: string) => {
      setError(message)
      setAnswer('')
      setAskDictationState('idle')
      setBusy(false)
    },
    [setAskDictationState, setBusy],
  )

  const beginDictation = useCallback(async () => {
    if (loadingRef.current || dictationStateRef.current !== 'idle') return

    setAnswer('')
    setError('')
    setAskDictationState('recording')
    try {
      await startAskDictation()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
      setAskDictationState('idle')
    }
  }, [setAskDictationState])

  const finishDictation = useCallback(async () => {
    if (loadingRef.current || dictationStateRef.current !== 'recording') return

    setAskDictationState('processing')
    setBusy(true)
    setError('')
    try {
      const result = await stopAskDictation()
      setAnswer(result.answer)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
      setAskDictationState('idle')
    }
  }, [setAskDictationState, setBusy])

  useEffect(() => {
    let cancelled = false
    const unlisteners: Array<() => void> = []
    const applyPendingMessage = async () => {
      const pending = await takePendingAskMessage()
      if (cancelled || !pending) return
      if (pending.kind === 'result') {
        applyResult(pending.payload)
      } else {
        applyError(pending.payload)
      }
    }

    import('@tauri-apps/api/event')
      .then(({ listen }) =>
        Promise.all([
          listen<AskResultPayload>('ask:result', (event) => {
            if (!cancelled) {
              applyResult(event.payload)
              void takePendingAskMessage().catch(() => {})
            }
          }),
          listen<string>('ask:error', (event) => {
            if (!cancelled) {
              applyError(event.payload)
              void takePendingAskMessage().catch(() => {})
            }
          }),
        ]),
      )
      .then((listeners) => {
        if (cancelled) {
          listeners.forEach((unlisten) => unlisten())
        } else {
          unlisteners.push(...listeners)
          void applyPendingMessage().catch(() => {})
        }
      })
      .catch(() => {})

    return () => {
      cancelled = true
      unlisteners.forEach((unlisten) => unlisten())
      Promise.resolve(abortAskDictation()).catch(() => {})
    }
  }, [applyError, applyResult])

  const toggleDictation = useCallback(() => {
    if (dictationState === 'recording') {
      void finishDictation()
      return
    }

    void beginDictation()
  }, [beginDictation, dictationState, finishDictation])

  const capsuleLabel =
    dictationState === 'recording'
      ? t('ask.listening')
      : dictationState === 'processing'
        ? t('ask.thinking')
        : t('ask.ready')
  const capsuleActive = dictationState === 'recording' || dictationState === 'processing'
  const displayTitle = title === 'Ask' ? t('ask.title') : title
  const resultText = error || answer

  if (!embedded) {
    return (
      <div className="h-screen w-screen overflow-y-auto bg-bg-primary px-4 py-3 text-text-primary">
        {resultText && (
          <p
            className={`whitespace-pre-wrap text-[13px] leading-5 ${
              error ? 'text-error' : 'text-text-primary'
            }`}
          >
            {resultText}
          </p>
        )}
      </div>
    )
  }

  return (
    <div
      className={`${embedded ? 'w-full' : 'h-screen w-screen'} bg-bg-primary text-text-primary flex flex-col`}
    >
      {showHeader && (
        <div className="flex items-center justify-between border-b border-border px-3 py-2">
          <span className="text-[13px] font-medium">{displayTitle}</span>
        </div>
      )}

      <div className={`${embedded ? 'p-3' : 'flex-1 min-h-0 p-3'} flex flex-col gap-3`}>
        <button
          type="button"
          aria-label={
            dictationState === 'recording' ? t('ask.stopAndAsk') : t('ask.recordQuestion')
          }
          onClick={toggleDictation}
          disabled={loading && dictationState !== 'recording'}
          className={`h-11 rounded-full border px-4 text-[13px] font-medium cursor-pointer disabled:cursor-not-allowed disabled:opacity-50 flex items-center gap-2 transition-colors ${
            capsuleActive
              ? 'bg-accent text-white border-accent shadow-sm'
              : 'bg-bg-secondary text-text-primary border-border hover:border-border-focus'
          }`}
        >
          {dictationState === 'processing' ? (
            <Loader2 size={14} className="animate-spin" />
          ) : dictationState === 'recording' ? (
            <span className="h-2 w-2 rounded-full bg-white animate-pulse" />
          ) : (
            <Mic size={14} />
          )}
          <span className="flex-1 text-left">
            <span className="block text-[13px]">{t('ask.voiceQuestion')}</span>
            <span
              className={`block text-[11px] font-normal ${
                capsuleActive ? 'text-white/70' : 'text-text-tertiary'
              }`}
            >
              {capsuleLabel}
            </span>
          </span>
          {dictationState === 'recording' && <Square size={13} />}
        </button>

        <p className="text-[11px] text-text-tertiary -mt-1">{t('ask.voiceQuestionDesc')}</p>

        {resultText && (
          <div className="min-h-0 flex-1 overflow-y-auto rounded-[8px] border border-border bg-bg-secondary px-3 py-2">
            <p
              className={`text-[13px] leading-5 whitespace-pre-wrap ${
                error ? 'text-error' : 'text-text-primary'
              }`}
            >
              {resultText}
            </p>
          </div>
        )}
      </div>
    </div>
  )
}
