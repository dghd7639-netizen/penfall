import { describe, expect, it, vi, beforeEach } from 'vitest'
import { invoke } from '@tauri-apps/api/core'
import { askAnything, updateAskHotkey } from '../tauri'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}))

describe('Ask Anything Tauri wrappers', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('invokes the ask_anything command with the trimmed question', async () => {
    vi.mocked(invoke).mockResolvedValueOnce('A short answer.')

    const answer = await askAnything('  What is OpenTypeless?  ')

    expect(answer).toBe('A short answer.')
    expect(invoke).toHaveBeenCalledWith('ask_anything', { question: 'What is OpenTypeless?' })
  })

  it('invokes the ask hotkey update command independently from dictation hotkey', async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined)

    await updateAskHotkey('Ctrl+Shift+/')

    expect(invoke).toHaveBeenCalledWith('update_ask_hotkey', { hotkey: 'Ctrl+Shift+/' })
  })
})
