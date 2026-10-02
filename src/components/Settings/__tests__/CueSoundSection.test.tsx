import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { CueSoundSection } from '../GeneralPane'
import { CUE_SOUNDS } from '../../../lib/constants'
import { useAppStore } from '../../../stores/appStore'
import { previewCueSound } from '../../../lib/tauri'

vi.mock('../../../lib/tauri', () => ({
  previewCueSound: vi.fn().mockResolvedValue(undefined),
  captureMouseButton: vi.fn(),
  getPlatformCapabilities: vi.fn().mockResolvedValue(null),
  getHotkeyStatus: vi.fn().mockResolvedValue(null),
  resumeHotkey: vi.fn().mockResolvedValue(undefined),
}))

describe('CueSoundSection', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useAppStore.setState({ config: { ...useAppStore.getState().config, cue_sound: 'water-drop' } })
  })
  afterEach(cleanup)

  it('offers the three new sounds, the default and an off switch', () => {
    render(<CueSoundSection />)
    expect(CUE_SOUNDS.map((sound) => sound.id)).toEqual([
      'water-drop',
      'wood-soft',
      'warm-low',
      'gentle-drop',
      'off',
    ])
    for (const label of ['水滴', '木块·柔', '低沉暖音', '轻水滴', '关闭']) {
      expect(screen.getByText(label)).toBeInTheDocument()
    }
    expect(screen.getByRole('radio', { name: /^水滴/ })).toBeChecked()
  })

  it('selecting a sound stores it and plays a preview', () => {
    render(<CueSoundSection />)
    fireEvent.click(screen.getByRole('radio', { name: /木块·柔/ }))
    expect(useAppStore.getState().config.cue_sound).toBe('wood-soft')
    expect(previewCueSound).toHaveBeenCalledWith('wood-soft')
  })

  it('the play button previews without changing the selection', () => {
    render(<CueSoundSection />)
    fireEvent.click(screen.getByRole('button', { name: '试听低沉暖音' }))
    expect(previewCueSound).toHaveBeenCalledWith('warm-low')
    expect(useAppStore.getState().config.cue_sound).toBe('water-drop')
  })

  it('off is selectable, silent and has no preview button', () => {
    render(<CueSoundSection />)
    fireEvent.click(screen.getByRole('radio', { name: /关闭/ }))
    expect(useAppStore.getState().config.cue_sound).toBe('off')
    expect(previewCueSound).not.toHaveBeenCalled()
    expect(screen.queryByRole('button', { name: '试听关闭' })).toBeNull()
  })

  it('treats an unknown stored value as the default', () => {
    useAppStore.setState({ config: { ...useAppStore.getState().config, cue_sound: 'mystery' } })
    render(<CueSoundSection />)
    expect(screen.getByRole('radio', { name: /^水滴/ })).toBeChecked()
  })
})
