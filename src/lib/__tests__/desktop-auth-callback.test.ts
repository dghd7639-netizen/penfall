import { beforeEach, describe, expect, it, vi } from 'vitest'

describe('createDesktopAuthCallbackURL', () => {
  beforeEach(() => {
    vi.resetModules()
    vi.stubGlobal('crypto', { randomUUID: vi.fn(() => 'desktop-state-1') })
    localStorage.clear()
  })

  it('uses one callback query parameter so email verification links keep state intact', async () => {
    const { createDesktopAuthCallbackURL } = await import('../desktop-auth-callback')

    expect(createDesktopAuthCallbackURL()).toBe(
      'https://www.opentypeless.com/auth/callback?desktop=desktop-state-1',
    )
  })
})
