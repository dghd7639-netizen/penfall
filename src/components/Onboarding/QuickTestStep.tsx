import { useAppStore } from '../../stores/appStore'

export function QuickTestStep() {
  const config = useAppStore((s) => s.config)
  const pipelineState = useAppStore((s) => s.pipelineState)
  const partialTranscript = useAppStore((s) => s.partialTranscript)
  const finalTranscript = useAppStore((s) => s.finalTranscript)
  return (
    <div className="space-y-4 text-[13px]">
      <p className="text-text-secondary">
        先点下面的输入框，再{config.hotkey_mode === 'hold' ? '按住' : '按下'} {config.hotkey}{' '}
        说一句中文；{config.hotkey_mode === 'hold' ? '松开后' : '再按一次后'}
        ，检查文字是否进入输入框。
      </p>
      <label className="block space-y-2">
        <span>试用输入框</span>
        <textarea
          className="w-full min-h-28 rounded-[10px] border border-border bg-bg-secondary p-3 text-text-primary"
          placeholder="请先点击这里，再按快捷键讲话"
        />
      </label>
      <p role="status" className="text-text-tertiary">
        当前状态：{pipelineState === 'idle' ? '等待讲话' : pipelineState}
      </p>
      {(partialTranscript || finalTranscript) && (
        <p className="text-text-secondary">识别内容：{finalTranscript || partialTranscript}</p>
      )}
      <p className="text-text-tertiary text-[12px]">
        这是实际录音试用，会调用你选择的识别服务并可能产生费用。若文字只复制到了剪贴板，可到设置中切换输出方式。请先确认麦克风和辅助功能权限。
      </p>
    </div>
  )
}
