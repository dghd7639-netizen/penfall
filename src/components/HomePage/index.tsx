import { useTranslation } from 'react-i18next'
import { useAppStore } from '../../stores/appStore'
import type { DictationTiming } from '../../stores/appStore'
import { useRoute } from '../../lib/router'
import { LLM_PROVIDERS, STT_PROVIDERS } from '../../lib/constants'

const ALIYUN_STT = new Set(['aliyun-qwen3-asr', 'aliyun-qwen-audio'])

function seconds(ms: number) {
  return `${(ms / 1000).toFixed(1)} 秒`
}

/** The stage that took longest, so the user knows what to change. */
function slowestStage(timing: DictationTiming): { label: string; hint: string } {
  const stages = [
    { label: '识别', ms: timing.sttMs, hint: '识别变慢通常是网络或代理的问题。' },
    {
      label: '润色',
      ms: timing.llmMs,
      hint: '润色最慢：换成不带推理的轻量模型，或关闭润色，可以明显变快。',
    },
    { label: '输出', ms: timing.outputMs, hint: '输出变慢通常是目标应用接收文字较慢。' },
  ]
  const slowest = stages.reduce((a, b) => (b.ms > a.ms ? b : a))
  return { label: slowest.label, hint: slowest.hint }
}

function TimingCard() {
  const timings = useAppStore((s) => s.recentTimings)
  if (timings.length === 0) {
    return (
      <section className="jelly-card rounded-[18px] p-5 text-[13px] space-y-2">
        <h3 className="font-semibold">听写耗时</h3>
        <p className="text-text-tertiary">听写一次后，这里会显示识别、润色、输出各花了多少时间。</p>
      </section>
    )
  }
  const latest = timings[0]
  const slowest = slowestStage(latest)
  return (
    <section className="jelly-card rounded-[18px] p-5 text-[13px] space-y-3">
      <h3 className="font-semibold">听写耗时</h3>
      <div className="grid grid-cols-4 gap-3 text-center">
        {[
          { label: '识别', ms: latest.sttMs },
          { label: '润色', ms: latest.llmMs },
          { label: '输出', ms: latest.outputMs },
          { label: '合计', ms: latest.totalMs },
        ].map(({ label, ms }) => (
          <div key={label}>
            <p className="text-text-tertiary text-[12px]">{label}</p>
            <p
              className={`text-[18px] font-semibold ${label === slowest.label ? 'text-accent' : ''}`}
            >
              {seconds(ms)}
            </p>
          </div>
        ))}
      </div>
      {latest.totalMs > 3000 && <p className="text-text-secondary">{slowest.hint}</p>}
      {timings.length > 1 && (
        <p className="text-[12px] text-text-tertiary">
          最近 {timings.length} 次合计：
          {timings.map((timing) => seconds(timing.totalMs)).join(' · ')}
        </p>
      )}
    </section>
  )
}

export function HomePage() {
  const { t } = useTranslation()
  const config = useAppStore((s) => s.config)
  const history = useAppStore((s) => s.history)
  const { navigate } = useRoute()
  const sttLabel = ALIYUN_STT.has(config.stt_provider)
    ? `${t('providers.stt.aliyun')} · ${
        config.stt_provider === 'aliyun-qwen-audio' ? 'Qwen-Audio 3.0' : 'Qwen3'
      }`
    : t(
        STT_PROVIDERS.find((provider) => provider.value === config.stt_provider)?.labelKey ??
          'providers.stt.aliyun',
      )
  const llmLabel = LLM_PROVIDERS.find((provider) => provider.value === config.llm_provider)
  return (
    <div className="p-6 space-y-5 max-w-3xl mx-auto">
      <section className="jelly-card rounded-[18px] p-6 space-y-3">
        <p className="text-accent text-[12px]">说出来，写下来</p>
        <h2 className="text-[24px] font-semibold">
          {config.hotkey_mode === 'hold' ? '按住' : '按下'} {config.hotkey} 开始说话
        </h2>
        <p className="text-text-secondary text-[14px] leading-relaxed">
          {config.hotkey_mode === 'hold' ? '松开按键后' : '再按一次结束后'}，文字会完成转写
          {config.polish_enabled ? '和轻度整理' : ''}，输入到当前光标位置。消息不会自动发送。
        </p>
        <p className="text-[12px] text-text-tertiary">
          适用于 AI 对话、微信、推文草稿和浏览器搜索框。
        </p>
      </section>
      <TimingCard />
      <section className="jelly-card rounded-[18px] p-5 text-[13px] space-y-3">
        <h3 className="font-semibold">当前配置</h3>
        <p>识别：{sttLabel}</p>
        <p>
          整理：
          {config.polish_enabled
            ? `${llmLabel ? t(llmLabel.labelKey) : config.llm_provider} · ${config.llm_model}`
            : '关闭，直接输出转写'}
        </p>
        <p>
          历史：
          {config.history_enabled
            ? `本机保留 ${config.history_retention_days || '不限'} 天，最多 ${config.history_max_entries} 条`
            : '已关闭'}
        </p>
        <p>已加载 {history.length} 条最近记录</p>
      </section>
      <div className="flex gap-3">
        <button onClick={() => navigate('settings')} className="jelly-btn rounded-full px-5 py-3">
          调整设置
        </button>
        <button onClick={() => navigate('history')} className="jelly-btn rounded-full px-5 py-3">
          查看历史
        </button>
      </div>
    </div>
  )
}
