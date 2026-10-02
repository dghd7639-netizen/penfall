import { cleanup, render } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { useAppStore } from '../../../stores/appStore'
import { Waveform } from '../Waveform'

const preference = vi.hoisted(() => ({ reduced: false }))
vi.mock('framer-motion', () => ({ useReducedMotion: () => preference.reduced }))

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
  useAppStore.setState(useAppStore.getInitialState())
  preference.reduced = false
})

describe('recorder trace', () => {
  it('shows quiet input, stays within its viewBox and cancels sampling on unmount', () => {
    let callback: FrameRequestCallback = () => {}
    vi.stubGlobal(
      'requestAnimationFrame',
      vi.fn((next: FrameRequestCallback) => {
        callback = next
        return 1
      }),
    )
    const cancel = vi.fn()
    vi.stubGlobal('cancelAnimationFrame', cancel)
    const { container, unmount } = render(<Waveform />)
    const path = container.querySelector('path')!
    useAppStore.setState({ audioVolume: 0.004 })
    callback(50)
    const points = path
      .getAttribute('d')!
      .match(/[-\d.]+/g)!
      .map(Number)
    expect(Math.abs(points[points.length - 1] - 10)).toBeGreaterThan(1)
    for (const [i, value] of points.entries()) {
      expect(value).toBeGreaterThanOrEqual(0)
      expect(value).toBeLessThanOrEqual(i % 2 === 0 ? 64 : 20)
    }
    for (const [index, volume] of [1, -1, NaN, Infinity].entries()) {
      useAppStore.setState({ audioVolume: volume })
      callback(100 + index * 50)
      expect(path.getAttribute('d')).not.toMatch(/NaN|Infinity/)
    }
    unmount()
    expect(cancel).toHaveBeenCalledWith(1)
  })

  it('keeps the trace still without scheduling frames under reduced motion', () => {
    preference.reduced = true
    const request = vi.fn()
    vi.stubGlobal('requestAnimationFrame', request)
    const { container } = render(<Waveform />)
    expect(container.querySelector('path')).toHaveAttribute('d', 'M0 10 L64 10')
    expect(request).not.toHaveBeenCalled()
  })
})
