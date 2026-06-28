import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../../../i18n'
import { AskPanel } from '../AskPanel'
import { abortAskDictation, startAskDictation, stopAskDictation } from '../../../lib/tauri'

const tauriEventMock = vi.hoisted(() => {
  type Listener = (event: { payload: unknown }) => void
  const listeners = new Map<string, Listener[]>()
  return {
    listeners,
    listen: vi.fn((event: string, callback: Listener) => {
      const current = listeners.get(event) ?? []
      current.push(callback)
      listeners.set(event, current)
      return Promise.resolve(() => {
        listeners.set(
          event,
          (listeners.get(event) ?? []).filter((listener) => listener !== callback),
        )
      })
    }),
    emit(event: string, payload?: unknown) {
      for (const listener of listeners.get(event) ?? []) {
        listener({ payload })
      }
    },
  }
})

vi.mock('../../../lib/tauri', () => ({
  startAskDictation: vi.fn(),
  stopAskDictation: vi.fn(),
  abortAskDictation: vi.fn(),
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: tauriEventMock.listen,
}))

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  tauriEventMock.listeners.clear()
})

describe('AskPanel', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en')
    vi.mocked(startAskDictation).mockResolvedValue(undefined)
    vi.mocked(stopAskDictation).mockResolvedValue({
      question: 'What is OpenTypeless?',
      answer: 'It turns speech into useful text.',
    })
    vi.mocked(abortAskDictation).mockResolvedValue(undefined)
  })

  it('renders the hotkey result as answer-only popup content', async () => {
    render(<AskPanel />)

    expect(screen.queryByRole('textbox')).toBeNull()
    expect(screen.queryByRole('button')).toBeNull()

    await waitFor(() => {
      expect(tauriEventMock.listen).toHaveBeenCalledWith('ask:result', expect.any(Function))
    })
    tauriEventMock.emit('ask:result', {
      question: 'What is OpenTypeless?',
      answer: 'It turns speech into useful text.',
    })

    await waitFor(() => {
      expect(screen.getByText('It turns speech into useful text.')).toBeDefined()
    })
    expect(screen.queryByRole('textbox')).toBeNull()
    expect(screen.queryByRole('button')).toBeNull()
    expect(screen.queryByText('Answer')).toBeNull()
    expect(startAskDictation).not.toHaveBeenCalled()
  })

  it('records a spoken question, asks the model, and renders the answer', async () => {
    render(<AskPanel embedded />)

    fireEvent.click(screen.getByRole('button', { name: 'Record question' }))
    await waitFor(() => expect(startAskDictation).toHaveBeenCalledTimes(1))
    fireEvent.click(screen.getByRole('button', { name: 'Stop and ask' }))

    await waitFor(() => {
      expect(screen.getByText('It turns speech into useful text.')).toBeDefined()
    })
    expect(stopAskDictation).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole('textbox')).toBeNull()
    expect(screen.queryByRole('button', { name: 'Ask' })).toBeNull()
  })

  it('renders backend errors as popup content only', async () => {
    render(<AskPanel />)

    await waitFor(() => {
      expect(tauriEventMock.listen).toHaveBeenCalledWith('ask:error', expect.any(Function))
    })
    tauriEventMock.emit('ask:error', 'Cloud AI quota exceeded.')

    await waitFor(() => {
      expect(screen.getByText('Cloud AI quota exceeded.')).toBeDefined()
    })
    expect(screen.queryByRole('textbox')).toBeNull()
    expect(screen.queryByRole('button')).toBeNull()
    expect(screen.queryByText('Error')).toBeNull()
  })

  it('uses localized copy for the voice-first ask flow', async () => {
    await i18n.changeLanguage('zh')
    render(<AskPanel embedded />)

    expect(screen.getByText('准备提问')).toBeDefined()
    expect(screen.getByText('说出问题，停止后自动回答')).toBeDefined()

    fireEvent.click(screen.getByRole('button', { name: '录制问题' }))
    await waitFor(() => expect(screen.getByText('正在聆听')).toBeDefined())
    expect(screen.getByRole('button', { name: '停止并提问' })).toBeDefined()
  })
})
