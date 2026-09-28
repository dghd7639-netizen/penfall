import React from 'react'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { Onboarding } from '../index'
import { getConfig, saveOnboardingCompleted, updateConfig } from '../../../lib/tauri'

const mockStore = {
  onboardingStep: 0,
  setOnboardingStep: vi.fn((step: number) => {
    mockStore.onboardingStep = step
  }),
  setOnboardingCompleted: vi.fn(),
  config: {},
  setConfig: vi.fn(),
  setSavedConfig: vi.fn(),
}
vi.mock('framer-motion', () => ({
  AnimatePresence: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}))
vi.mock('../OnboardingLayout', () => ({
  OnboardingLayout: ({
    children,
    onNext,
    onBack,
    canNext,
  }: {
    children: React.ReactNode
    onNext: () => void
    onBack: () => void
    canNext: boolean
  }) => (
    <div>
      <button onClick={onNext} disabled={!canNext}>
        Next
      </button>
      <button onClick={onBack}>Back</button>
      {children}
    </div>
  ),
}))
vi.mock('../../Settings/DashscopeSetup', () => ({
  DashscopeSetup: ({ onSaved }: { onSaved: () => void }) => (
    <button onClick={onSaved}>Save service</button>
  ),
}))
vi.mock('../PermissionsStep', () => ({ PermissionsStep: () => <div>Permissions</div> }))
vi.mock('../QuickTestStep', () => ({ QuickTestStep: () => <div>Actual test</div> }))
vi.mock('../../../stores/appStore', () => ({
  useAppStore: Object.assign(
    (selector: (state: typeof mockStore) => unknown) => selector(mockStore),
    { getState: () => mockStore },
  ),
}))
vi.mock('../../../lib/tauri', () => ({
  updateConfig: vi.fn().mockResolvedValue(undefined),
  getConfig: vi.fn().mockResolvedValue({}),
  saveOnboardingCompleted: vi.fn().mockResolvedValue(undefined),
}))
beforeEach(() => {
  mockStore.onboardingStep = 0
  vi.clearAllMocks()
  vi.mocked(updateConfig).mockResolvedValue(undefined)
  vi.mocked(getConfig).mockResolvedValue({} as never)
  vi.mocked(saveOnboardingCompleted).mockResolvedValue(undefined)
})
afterEach(() => cleanup())
describe('personal onboarding', () => {
  it('requires service setup before advancing to permissions', () => {
    render(<Onboarding />)
    expect(screen.getByRole('button', { name: 'Next' })).toBeDisabled()
    fireEvent.click(screen.getByRole('button', { name: 'Save service' }))
    expect(mockStore.setOnboardingStep).toHaveBeenCalledWith(1)
  })
  it('returns from permissions to service setup', () => {
    mockStore.onboardingStep = 1
    render(<Onboarding />)
    fireEvent.click(screen.getByRole('button', { name: 'Back' }))
    expect(mockStore.setOnboardingStep).toHaveBeenCalledWith(0)
  })
  it('does not mark onboarding complete after a failed save', async () => {
    mockStore.onboardingStep = 2
    vi.mocked(updateConfig).mockRejectedValue(new Error('disk error'))
    render(<Onboarding />)
    fireEvent.click(screen.getByRole('button', { name: 'Next' }))
    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent('设置保存失败'))
    expect(mockStore.setOnboardingCompleted).not.toHaveBeenCalled()
  })
  it('completes only after saving and reading back configuration', async () => {
    mockStore.onboardingStep = 2
    render(<Onboarding />)
    fireEvent.click(screen.getByRole('button', { name: 'Next' }))
    await waitFor(() => expect(mockStore.setOnboardingCompleted).toHaveBeenCalledWith(true))
    expect(updateConfig).toHaveBeenCalledWith(mockStore.config)
    expect(getConfig).toHaveBeenCalledOnce()
    expect(saveOnboardingCompleted).toHaveBeenCalledOnce()
  })
})
