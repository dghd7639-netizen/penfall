import React from 'react'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { invoke } from '@tauri-apps/api/core'
import { emit } from '@tauri-apps/api/event'
import { useAppStore } from '../../../stores/appStore'
import { removeCorrectionRule, setActiveTranslationTarget, stopAskFlow } from '../../../lib/tauri'
import { Capsule } from '../index'
import { LEARNED_NOTICE_MS } from '../CapsuleLearned'
import { writeText } from '@tauri-apps/plugin-clipboard-manager'

vi.mock('framer-motion', () => {
  // Cache per tag so motion.div keeps one component identity across renders, like the real library.
  const components = new Map<string, React.FC<React.HTMLAttributes<HTMLElement>>>()
  return {
    AnimatePresence: ({ children }: { children: React.ReactNode }) => <>{children}</>,
    motion: new Proxy(
      {},
      {
        get: (_target, tag: string) => {
          if (!components.has(tag)) {
            components.set(tag, ({ children, ...props }) =>
              React.createElement(tag, props, children),
            )
          }
          return components.get(tag)
        },
      },
    ),
    useReducedMotion: () => true,
  }
})

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

vi.mock('../../../hooks/useCapsuleResize', () => ({
  DICTATION_WIDTH: 160,
  RESCUE_WIDTH: 320,
  useCapsuleResize: () => ({ width: 160, height: 36 }),
}))

vi.mock('../../../lib/tauri', () => ({
  abortAskDictation: vi.fn().mockResolvedValue(undefined),
  abortRecording: vi.fn().mockResolvedValue(undefined),
  setActiveTranslationTarget: vi.fn().mockResolvedValue({
    targets: ['en', 'ja'],
    active_target: 'ja',
  }),
  removeCorrectionRule: vi.fn().mockResolvedValue(undefined),
  setCapsuleAutoHide: vi.fn().mockResolvedValue(undefined),
  stopAskFlow: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({
  writeText: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('@tauri-apps/api/event', () => ({
  emit: vi.fn().mockResolvedValue(undefined),
  listen: vi.fn().mockResolvedValue(() => {}),
}))

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  useAppStore.setState(useAppStore.getInitialState())
})

describe('Capsule flow states', () => {
  beforeEach(() => {
    useAppStore.setState({
      pipelineState: 'idle',
      pipelineError: null,
      contextMenuOpen: false,
      contextMenuReady: false,
      translationTargetMenuOpen: false,
      activeVoiceMode: null,
      partialTranscript: '',
    })
  })

  it('shows the recording capsule while preparing instead of a preparing message', () => {
    useAppStore.setState({ pipelineState: 'preparing' })

    const { container } = render(<Capsule />)

    expect(screen.queryByText('capsule.preparing')).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'capsule.cancelRecording' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'capsule.finishRecording' })).toBeInTheDocument()
    const shell = container.querySelector('.jelly-capsule-active') as HTMLElement
    expect(shell.style.width).toBe('160px')
  })

  it('keeps one progress fill running from transcribing through polishing', () => {
    useAppStore.setState({ pipelineState: 'transcribing' })
    render(<Capsule />)
    const fill = screen.getByTestId('capsule-progress')
    expect(screen.getByText('capsule.transcribing')).toBeInTheDocument()

    act(() => {
      useAppStore.setState({ pipelineState: 'polishing' })
    })

    expect(screen.getByTestId('capsule-progress')).toBe(fill)
    expect(screen.getByText('capsule.thinking')).toBeInTheDocument()
  })

  it('keeps the capsule the same length from recording through completion', () => {
    useAppStore.setState({ pipelineState: 'recording' })
    const { container } = render(<Capsule />)
    const shell = () => container.querySelector('.jelly-capsule-active') as HTMLElement

    for (const pipelineState of ['recording', 'transcribing', 'polishing', 'outputting'] as const) {
      act(() => {
        useAppStore.setState({ pipelineState })
      })
      expect(shell().style.width, pipelineState).toBe('160px')
    }
  })

  it('shows no progress fill while recording', () => {
    useAppStore.setState({ pipelineState: 'recording' })
    render(<Capsule />)
    expect(screen.queryByTestId('capsule-progress')).not.toBeInTheDocument()
  })

  it('shows learned corrections one at a time, each with its own undo', async () => {
    useAppStore.setState({
      learnedCorrections: [
        { id: 7, pattern: '同意', replacement: '通义' },
        { id: 8, pattern: '拓肯', replacement: 'Token' },
      ],
    })

    const { container } = render(<Capsule />)

    expect(container.textContent).toContain('同意 → 通义')
    expect(container.textContent).not.toContain('拓肯')
    const shell = container.querySelector('.jelly-capsule-active') as HTMLElement
    expect(shell.style.width).toBe('220px')
    fireEvent.click(screen.getByRole('button', { name: 'capsule.undoLearnedCorrection' }))
    await waitFor(() => expect(container.textContent).toContain('capsule.undoneLearnedCorrection'))
    expect(removeCorrectionRule).toHaveBeenCalledWith(7)
    expect(removeCorrectionRule).not.toHaveBeenCalledWith(8)
    expect(emit).toHaveBeenCalledWith('dictionary:changed')
    await waitFor(() => expect(container.textContent).toContain('拓肯 → Token'), {
      timeout: 3000,
    })
    expect(useAppStore.getState().learnedCorrections.map((c) => c.id)).toEqual([8])
  })

  it('dismisses a learned correction notice on its own and keeps the rule', () => {
    vi.useFakeTimers()
    try {
      useAppStore.setState({
        learnedCorrections: [{ id: 7, pattern: '同意', replacement: '通义' }],
      })
      render(<Capsule />)

      act(() => {
        vi.advanceTimersByTime(LEARNED_NOTICE_MS)
      })

      expect(useAppStore.getState().learnedCorrections).toEqual([])
      expect(removeCorrectionRule).not.toHaveBeenCalled()
    } finally {
      vi.useRealTimers()
    }
  })

  it('shows recording instead of a learned correction notice once dictation starts', () => {
    useAppStore.setState({
      pipelineState: 'recording',
      learnedCorrections: [{ id: 7, pattern: '同意', replacement: '通义' }],
    })

    const { container } = render(<Capsule />)

    expect(container.textContent).not.toContain('通义')
    expect(screen.getByRole('button', { name: 'capsule.finishRecording' })).toBeInTheDocument()
  })

  it('keeps text that was not typed up until it is copied', async () => {
    useAppStore.setState({
      pipelineError: 'capsule.errors.llm_failed',
      rescue: { text: '第一行\n第二行', reason: 'capsule.errors.llm_failed' },
    })

    const { container } = render(<Capsule />)

    expect(container.textContent).toContain('第一行 第二行')
    const shell = container.querySelector('.jelly-capsule-active') as HTMLElement
    expect(shell.style.width).toBe('320px')
    fireEvent.click(screen.getByRole('button', { name: 'capsule.rescueCopy' }))
    await waitFor(() => expect(container.textContent).toContain('capsule.rescueCopied'))
    expect(writeText).toHaveBeenCalledWith('第一行\n第二行')
    await waitFor(() => expect(useAppStore.getState().rescue).toBeNull(), { timeout: 3000 })
    expect(useAppStore.getState().pipelineError).toBeNull()
  })

  it('dismisses rescued text without copying it', () => {
    useAppStore.setState({ rescue: { text: '没输进去的话', reason: null } })
    render(<Capsule />)

    fireEvent.click(screen.getByRole('button', { name: 'capsule.rescueDismiss' }))

    expect(useAppStore.getState().rescue).toBeNull()
    expect(writeText).not.toHaveBeenCalled()
  })

  it('finishes recording only from the check button, not the draggable shell', async () => {
    useAppStore.setState({ pipelineState: 'recording' })
    const { container } = render(<Capsule />)
    const shell = container.querySelector('.jelly-capsule-active') as HTMLElement
    const pointerUp = new Event('pointerup', { bubbles: true })
    Object.defineProperty(pointerUp, 'button', { value: 0 })
    fireEvent(shell, pointerUp)
    expect(invoke).not.toHaveBeenCalledWith('stop_recording')
    const finish = screen.getByRole('button', { name: 'capsule.finishRecording' })
    fireEvent.pointerDown(finish)
    fireEvent.pointerUp(finish)
    fireEvent.click(finish)
    await waitFor(() => expect(invoke).toHaveBeenCalledExactlyOnceWith('stop_recording'))
    expect(shell.style.width).toBe('160px')
  })

  it('renders transcribing state with partial transcript when available', () => {
    useAppStore.setState({
      pipelineState: 'transcribing',
      partialTranscript: 'hello world',
    })

    render(<Capsule />)

    expect(screen.getByText(/hello world/)).toBeInTheDocument()
  })

  it('renders thinking state during polishing', () => {
    useAppStore.setState({ pipelineState: 'polishing' })

    render(<Capsule />)

    expect(screen.getByText('capsule.thinking')).toBeInTheDocument()
  })

  it('does not start dictation when the idle capsule is clicked', () => {
    const { container } = render(<Capsule />)
    const shell = container.querySelector('.jelly-capsule')
    expect(shell).toBeTruthy()

    const pointerUp = new Event('pointerup', { bubbles: true })
    Object.defineProperty(pointerUp, 'button', { value: 0 })
    fireEvent(shell as Element, pointerUp)

    expect(invoke).not.toHaveBeenCalledWith('start_recording')
  })

  it('renders Ask recording in the capsule and stops Ask when clicked', () => {
    useAppStore.setState({ pipelineState: 'ask_recording' })

    render(<Capsule />)

    expect(screen.getByText('ask.title')).toBeInTheDocument()
    expect(screen.getByText('ask.title')).toHaveClass('whitespace-nowrap')
    expect(screen.getByText('00:00')).toBeInTheDocument()

    const pointerUp = new Event('pointerup', { bubbles: true })
    Object.defineProperty(pointerUp, 'button', { value: 0 })
    fireEvent(screen.getByText('ask.title'), pointerUp)

    expect(stopAskFlow).toHaveBeenCalledTimes(1)
  })

  it('renders Ask thinking in the capsule', () => {
    useAppStore.setState({ pipelineState: 'ask_thinking' })

    render(<Capsule />)

    expect(screen.getByText('ask.title')).toBeInTheDocument()
    expect(screen.getByText('ask.title')).toHaveClass('whitespace-nowrap')
    expect(screen.getByText('ask.thinking')).toBeInTheDocument()
  })

  it('shows the language chip only while Translate is recording', () => {
    useAppStore.setState({
      pipelineState: 'recording',
      activeVoiceMode: 'dictate',
      config: {
        ...useAppStore.getState().config,
        translation: { targets: ['en', 'ja'], active_target: 'en' },
      },
    })
    const { rerender } = render(<Capsule />)

    expect(screen.queryByRole('button', { name: 'capsule.translationTarget en' })).toBeNull()

    useAppStore.setState({ activeVoiceMode: 'translate' })
    rerender(<Capsule />)
    expect(screen.getByRole('button', { name: 'capsule.translationTarget en' })).toBeInTheDocument()

    useAppStore.setState({ pipelineState: 'transcribing' })
    rerender(<Capsule />)
    expect(screen.queryByRole('button', { name: 'capsule.translationTarget en' })).toBeNull()
  })

  it('switches the active target without restarting or stopping recording', async () => {
    useAppStore.setState({
      pipelineState: 'recording',
      activeVoiceMode: 'translate',
      config: {
        ...useAppStore.getState().config,
        translation: { targets: ['en', 'ja'], active_target: 'en' },
      },
    })
    render(<Capsule />)

    fireEvent.pointerDown(screen.getByRole('button', { name: 'capsule.translationTarget en' }))
    fireEvent.click(screen.getByRole('button', { name: 'capsule.translationTarget en' }))
    fireEvent.click(screen.getByRole('menuitemradio', { name: '日本語' }))

    await waitFor(() => expect(setActiveTranslationTarget).toHaveBeenCalledWith('ja'))
    expect(invoke).not.toHaveBeenCalledWith('stop_recording')
    expect(invoke).not.toHaveBeenCalledWith('start_recording')
    expect(useAppStore.getState().config.translation.active_target).toBe('ja')
  })

  it('keeps the capsule shell at 160 by 36 and closes the target menu on Escape', async () => {
    useAppStore.setState({
      pipelineState: 'recording',
      activeVoiceMode: 'translate',
      config: {
        ...useAppStore.getState().config,
        translation: { targets: ['en', 'ja'], active_target: 'en' },
      },
    })
    const { container } = render(<Capsule />)
    const chip = screen.getByRole('button', { name: 'capsule.translationTarget en' })

    fireEvent.click(chip)
    expect(screen.getByRole('menu', { name: 'capsule.translationTargets' })).toBeInTheDocument()

    fireEvent.keyDown(window, { key: 'Escape' })
    expect(screen.queryByRole('menu', { name: 'capsule.translationTargets' })).toBeNull()
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'capsule.translationTarget en' })).toHaveFocus(),
    )

    const shell = container.querySelector('.jelly-capsule-active') as HTMLElement
    expect(shell.style.width).toBe('160px')
    expect(shell.style.height).toBe('36px')
  })
})
