import { useState } from 'react'
import { useAppStore } from '../../stores/appStore'
import { getConfig, saveOnboardingCompleted, updateConfig as saveConfig } from '../../lib/tauri'
import { OnboardingLayout } from './OnboardingLayout'
import { DashscopeSetup } from '../Settings/DashscopeSetup'
import { PermissionsStep } from './PermissionsStep'
import { QuickTestStep } from './QuickTestStep'

const titles = ['配置中文语音服务', '允许录音与文字输入', '试说一句话']

export function Onboarding() {
  const step = useAppStore((s) => s.onboardingStep)
  const setStep = useAppStore((s) => s.setOnboardingStep)
  const setCompleted = useAppStore((s) => s.setOnboardingCompleted)
  const config = useAppStore((s) => s.config)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  async function next() {
    if (busy) return
    setBusy(true)
    setError('')
    try {
      await saveConfig(config)
      if (step === 2) {
        const saved = await getConfig()
        await saveOnboardingCompleted()
        useAppStore.getState().setConfig(saved)
        useAppStore.getState().setSavedConfig(saved)
        setCompleted(true)
      } else {
        setStep(step + 1)
      }
    } catch {
      setError('设置保存失败，请重试；尚未完成首次配置。')
    } finally {
      setBusy(false)
    }
  }
  return (
    <OnboardingLayout
      step={step}
      totalSteps={3}
      title={titles[step]}
      subtitle="Penfall · 中文语音输入"
      canNext={step > 0 && !busy}
      canBack={step > 0 && !busy}
      nextLabel={busy ? '保存中…' : step === 2 ? '开始使用' : '下一步'}
      onNext={() => void next()}
      onBack={() => {
        setError('')
        setStep(Math.max(0, step - 1))
      }}
    >
      {step === 0 && <DashscopeSetup onSaved={() => setStep(1)} />}
      {step === 1 && <PermissionsStep />}
      {step === 2 && <QuickTestStep />}
      {error && (
        <p role="alert" className="text-error mt-3">
          {error}
        </p>
      )}
    </OnboardingLayout>
  )
}
