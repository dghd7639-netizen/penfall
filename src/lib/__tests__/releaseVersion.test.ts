import { describe, expect, it } from 'vitest'
import constantsSource from '../constants.ts?raw'
import releaseWorkflowSource from '../../../.github/workflows/release.yml?raw'

describe('release version wiring', () => {
  it('lets frontend builds read the release tag version from Vite env', () => {
    expect(constantsSource).toContain('import.meta.env.VITE_APP_VERSION')
  })

  it('exports VITE_APP_VERSION during the GitHub release build', () => {
    expect(releaseWorkflowSource).toContain('VITE_APP_VERSION=v$VERSION')
  })
})
