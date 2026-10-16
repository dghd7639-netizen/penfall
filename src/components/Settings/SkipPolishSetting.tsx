import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '../../stores/appStore'
import { Toggle } from './shared/Toggle'

const MIN_CHARS = 4
const MAX_CHARS = 100

/** Lets clean short dictations skip AI polish, up to a length the user picks. */
export function SkipPolishSetting() {
  const { t } = useTranslation()
  const polishEnabled = useAppStore((s) => s.config.polish_enabled)
  const enabled = useAppStore((s) => s.config.skip_polish_when_clean)
  const maxChars = useAppStore((s) => s.config.skip_polish_max_chars)
  const updateConfig = useAppStore((s) => s.updateConfig)
  // Edited as text and committed on blur, so typing "45" never stops at the minimum.
  const [draft, setDraft] = useState<string | null>(null)

  const commit = () => {
    if (draft === null) return
    const parsed = Number.parseInt(draft, 10)
    if (Number.isFinite(parsed)) {
      updateConfig({
        skip_polish_max_chars: Math.min(MAX_CHARS, Math.max(MIN_CHARS, parsed)),
      })
    }
    setDraft(null)
  }

  const disabled = !polishEnabled || !enabled
  return (
    <div>
      <Toggle
        checked={enabled}
        disabled={!polishEnabled}
        onChange={(checked) => updateConfig({ skip_polish_when_clean: checked })}
        label={t('settings.skipPolishWhenClean')}
      />
      <div className="mt-2 ml-[52px] min-w-0 space-y-1">
        <label
          className={`flex items-center gap-2 text-[12px] text-text-secondary ${
            disabled ? 'opacity-50' : ''
          }`}
        >
          {t('settings.skipPolishMaxChars')}
          <input
            type="number"
            inputMode="numeric"
            min={MIN_CHARS}
            max={MAX_CHARS}
            disabled={disabled}
            value={draft ?? String(maxChars)}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={commit}
            onKeyDown={(e) => {
              if (e.key === 'Enter') e.currentTarget.blur()
            }}
            className="w-16 px-2 py-1 bg-bg-secondary border border-border rounded-[8px] text-[12px] text-text-primary outline-none focus:border-border-focus disabled:cursor-not-allowed"
          />
        </label>
        <p className="text-[11px] leading-relaxed text-text-tertiary">
          {t('settings.skipPolishWhenCleanHint')}
        </p>
      </div>
    </div>
  )
}
