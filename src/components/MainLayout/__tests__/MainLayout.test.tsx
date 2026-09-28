import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { MainLayout } from '../index'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) =>
      ({
        'nav.home': 'Home',
        'nav.settings': 'Settings',
        'nav.history': 'History',
        'nav.mainNavigation': 'Main navigation',
      })[key] ?? key,
  }),
}))
vi.mock('../AccessibilityBanner', () => ({
  AccessibilityBanner: () => <div>Accessibility status</div>,
}))
afterEach(() => cleanup())
describe('personal navigation', () => {
  it('shows home, settings, history and the local permission banner', () => {
    render(
      <MainLayout>
        <div>content</div>
      </MainLayout>,
    )
    expect(screen.getByRole('button', { name: 'Home' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Settings' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'History' })).toBeInTheDocument()
    expect(screen.getByText('Accessibility status')).toBeInTheDocument()
  })
  it('does not expose account, upgrade or hosted-cloud recovery', () => {
    render(
      <MainLayout>
        <div>content</div>
      </MainLayout>,
    )
    for (const name of ['Ask', 'Account', 'Upgrade'])
      expect(screen.queryByRole('button', { name })).not.toBeInTheDocument()
    expect(screen.queryByTestId('cloud-service-banner')).not.toBeInTheDocument()
  })
})
