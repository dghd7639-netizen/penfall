import { describe, expect, it } from 'vitest'
import { useAppStore } from '../../stores/appStore'
import { parseHash } from '../router'

describe('personal Chinese voice input', () => {
  it('starts with Chinese, hold-to-talk, 30-day local history and no command shortcuts', () => {
    const config = useAppStore.getInitialState().config
    expect(config.stt_provider).toBe('aliyun-qwen3-asr')
    expect(config.llm_provider).toBe('qwen')
    expect(config.ui_language).toBe('zh')
    expect(config.stt_language).toBe('zh')
    expect(config.hotkey_mode).toBe('hold')
    expect(config.hotkeys.dictationMode).toBe('hold')
    expect(config.ask_hotkey).toBe('')
    expect(config.hotkeys.askBindings).toEqual([])
    expect(config.hotkeys.translateBindings).toEqual([])
    expect(Object.values(config.voice_routing_flags)).toEqual([false, false, false, false])
    expect(config.history_enabled).toBe(true)
    expect(config.history_retention_days).toBe(30)
    expect(config.stt_api_key).toBe('')
    expect(config.llm_api_key).toBe('')
  })

  it('does not route old account or upgrade links to hosted services', () => {
    for (const route of ['account', 'upgrade', 'ask']) {
      window.location.hash = `#/${route}`
      expect(parseHash()).toBe('home')
    }
    window.location.hash = ''
  })
})
