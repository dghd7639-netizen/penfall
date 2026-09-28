import { useAppStore } from '../../stores/appStore'
import { useRoute } from '../../lib/router'

export function HomePage() {
  const config = useAppStore((s) => s.config)
  const history = useAppStore((s) => s.history)
  const { navigate } = useRoute()
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
      <section className="jelly-card rounded-[18px] p-5 text-[13px] space-y-3">
        <h3 className="font-semibold">当前配置</h3>
        <p>
          识别：
          {config.stt_provider === 'apple-speech'
            ? 'Apple Speech（系统备用）'
            : '阿里云 Qwen3 实时识别'}
        </p>
        <p>整理：{config.polish_enabled ? `Qwen · ${config.llm_model}` : '关闭，直接输出转写'}</p>
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
