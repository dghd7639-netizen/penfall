import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { SkipPolishSetting } from '../SkipPolishSetting'
import { useAppStore } from '../../../stores/appStore'

function setConfig(partial: Partial<ReturnType<typeof useAppStore.getState>['config']>) {
  useAppStore.setState({ config: { ...useAppStore.getState().config, ...partial } })
}

describe('SkipPolishSetting', () => {
  beforeEach(() => {
    setConfig({ polish_enabled: true, skip_polish_when_clean: true, skip_polish_max_chars: 30 })
  })
  afterEach(cleanup)

  it('is on by default for clean dictations up to 30 characters', () => {
    const defaults = useAppStore.getInitialState().config
    expect(defaults.skip_polish_when_clean).toBe(true)
    expect(defaults.skip_polish_max_chars).toBe(30)

    render(<SkipPolishSetting />)
    expect(screen.getByText('settings.skipPolishWhenClean')).toBeInTheDocument()
    expect(screen.getByRole('switch')).toHaveAttribute('aria-checked', 'true')
    expect(screen.getByRole('spinbutton')).toHaveValue(30)
  })

  it('turns the shortcut off', () => {
    render(<SkipPolishSetting />)
    fireEvent.click(screen.getByRole('switch'))
    expect(useAppStore.getState().config.skip_polish_when_clean).toBe(false)
  })

  it('keeps the length limit within 4 to 100 when the edit is committed', () => {
    render(<SkipPolishSetting />)
    const input = screen.getByRole('spinbutton')

    fireEvent.change(input, { target: { value: '1' } })
    expect(useAppStore.getState().config.skip_polish_max_chars).toBe(30)
    fireEvent.change(input, { target: { value: '15' } })
    fireEvent.blur(input)
    expect(useAppStore.getState().config.skip_polish_max_chars).toBe(15)

    fireEvent.change(input, { target: { value: '500' } })
    fireEvent.blur(input)
    expect(useAppStore.getState().config.skip_polish_max_chars).toBe(100)
  })

  it('is unavailable while AI polish is off', () => {
    setConfig({ polish_enabled: false })
    render(<SkipPolishSetting />)
    expect(screen.getByRole('switch')).toBeDisabled()
    expect(screen.getByRole('spinbutton')).toBeDisabled()
  })
})
