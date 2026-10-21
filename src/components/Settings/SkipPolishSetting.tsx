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
  // Edited as text. A value in range goes into the unsaved settings as it is
  // typed, so switching panes mid-edit keeps it ("45" passes through 4 on the
  // way, harmlessly: nothing takes effect until the settings are saved).
  const [draft, setDraft] = useState<string | null>(null)

  const handleChange = (value: string) => {
    setDraft(value)
    const parsed = Number.parseInt(value, 10)
    if (parsed >= MIN_CHARS && parsed <= MAX_CHARS) {
      updateConfig({ skip_polish_max_chars: parsed })
    }
  }

  // Out-of-range input is clamped when the field loses focus.
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
            onChange={(e) => handleChange(e.target.value)}
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
