import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { cleanup, render, screen } from '@testing-library/react'
import { HomePage } from '../index'
import { MAX_RECENT_TIMINGS, useAppStore } from '../../../stores/appStore'
import type { DictationTiming } from '../../../stores/appStore'

function timing(partial: Partial<DictationTiming>): DictationTiming {
  return {
    sttMs: 600,
    llmMs: 2000,
    outputMs: 700,
    totalMs: 3300,
    recordingMs: 5000,
    at: 0,
    ...partial,
  }
}

describe('HomePage dictation timing', () => {
  beforeEach(() => {
    useAppStore.setState({ recentTimings: [] })
  })
  afterEach(cleanup)

  it('explains that timings appear after the first dictation', () => {
    render(<HomePage />)
    expect(screen.getByText(/听写一次后/)).toBeInTheDocument()
  })

  it('shows each stage and points at polishing when it is the slowest', () => {
    useAppStore
      .getState()
      .addTiming(timing({ sttMs: 700, llmMs: 9000, outputMs: 800, totalMs: 10500 }))
    render(<HomePage />)

    expect(screen.getByText('0.7 秒')).toBeInTheDocument()
    expect(screen.getByText('9.0 秒')).toBeInTheDocument()
    expect(screen.getByText('0.8 秒')).toBeInTheDocument()
    expect(screen.getByText('10.5 秒')).toBeInTheDocument()
    expect(screen.getByText(/润色最慢/)).toBeInTheDocument()
  })

  it('points at recognition when that stage is the slowest', () => {
    useAppStore.getState().addTiming(timing({ sttMs: 8000, llmMs: 1800, totalMs: 10500 }))
    render(<HomePage />)
    expect(screen.getByText(/识别变慢/)).toBeInTheDocument()
  })

  it('gives no hint for a fast dictation', () => {
    useAppStore.getState().addTiming(timing({ totalMs: 2500 }))
    render(<HomePage />)
    expect(screen.queryByText(/润色最慢|识别变慢|输出变慢/)).toBeNull()
  })

  it('keeps only the most recent timings, newest first', () => {
    for (let i = 1; i <= MAX_RECENT_TIMINGS + 2; i++) {
      useAppStore.getState().addTiming(timing({ totalMs: i * 1000 }))
    }
    const timings = useAppStore.getState().recentTimings
    expect(timings).toHaveLength(MAX_RECENT_TIMINGS)
    expect(timings[0].totalMs).toBe((MAX_RECENT_TIMINGS + 2) * 1000)
  })
})
