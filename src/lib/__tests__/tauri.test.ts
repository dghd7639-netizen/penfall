import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { invoke } from '@tauri-apps/api/core'
import { waitForAccessibilityPermission } from '../tauri'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}))

describe('waitForAccessibilityPermission', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('returns immediately when accessibility is already trusted', async () => {
    vi.mocked(invoke).mockResolvedValueOnce(true)

    await expect(waitForAccessibilityPermission()).resolves.toBe(true)

    expect(invoke).toHaveBeenCalledTimes(1)
    expect(invoke).toHaveBeenCalledWith('check_accessibility_permission')
  })

  it('polls until accessibility becomes trusted', async () => {
    vi.useFakeTimers()
    vi.mocked(invoke).mockResolvedValueOnce(false).mockResolvedValueOnce(true)

    const result = waitForAccessibilityPermission({ timeoutMs: 1_000, intervalMs: 10 })
    await vi.advanceTimersByTimeAsync(10)

    await expect(result).resolves.toBe(true)
    expect(invoke).toHaveBeenCalledTimes(2)
  })

  it('returns false after the timeout expires', async () => {
    vi.useFakeTimers()
    vi.mocked(invoke).mockResolvedValue(false)

    const result = waitForAccessibilityPermission({ timeoutMs: 20, intervalMs: 10 })
    await vi.advanceTimersByTimeAsync(20)

    await expect(result).resolves.toBe(false)
    expect(invoke).toHaveBeenCalledTimes(3)
  })
})
