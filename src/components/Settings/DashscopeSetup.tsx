import { useEffect, useState } from 'react'
import { isMacPlatform, useAppStore } from '../../stores/appStore'
import { getCredentialStatus, testLlmConnection, testSttConnection } from '../../lib/tauri'
import { saveDashscopeSetup } from '../../lib/dashscope-setup'

export function DashscopeSetup({ onSaved }: { onSaved?: () => void }) {
  const config = useAppStore((s) => s.config)
  const [key, setKey] = useState('')
  const [hasKey, setHasKey] = useState(false)
  const [busy, setBusy] = useState(false)
  const [message, setMessage] = useState('')
  const [error, setError] = useState('')
  const update = useAppStore((s) => s.updateConfig)
  const setConfig = useAppStore((s) => s.setConfig)
  const setSavedConfig = useAppStore((s) => s.setSavedConfig)
  useEffect(() => {
    let active = true
    getCredentialStatus('stt', 'aliyun-qwen3-asr')
      .then((status) => {
        if (active) setHasKey(status.hasSecret)
      })
      .catch(() => {
        if (active) setError('暂时无法读取系统钥匙串，请允许访问后重试。')
      })
    return () => {
      active = false
    }
  }, [])

  async function save(test: boolean) {
    if (busy) return
    setBusy(true)
    setError('')
    setMessage('')
    try {
      const saved = await saveDashscopeSetup(config, key)
      if (key.trim()) setHasKey(true)
      setKey('')
      setConfig(saved)
      setSavedConfig(saved)
      if (test) {
        try {
          const sttOk = await testSttConnection(
            '',
            saved.stt_provider,
            undefined,
            undefined,
            undefined,
            saved.stt_aliyun_qwen_region,
          )
          if (!sttOk) throw new Error('stt')
          const llmOk =
            !saved.polish_enabled ||
            (await testLlmConnection('', 'qwen', saved.llm_base_url, saved.llm_model))
          if (!llmOk) throw new Error('llm')
          setMessage('连接成功。录音准确率和输入效果请在下一步试用。')
        } catch {
          setError('设置已保存，但连接未通过。请检查密钥所属地域、模型权限、网络及系统语音权限。')
          return
        }
      } else {
        setMessage('已保存。密钥只存储在系统钥匙串中；尚未进行在线连接测试。')
      }
      onSaved?.()
    } catch (e) {
      setError(e instanceof Error ? e.message : '保存失败，请重试。')
    } finally {
      setBusy(false)
    }
  }

  const inputStyle =
    'w-full rounded-[8px] border border-border bg-bg-secondary px-3 py-2 text-text-primary'
  return (
    <div className="space-y-4 text-[13px] max-w-xl">
      <p className="text-text-secondary">
        阿里云负责转写和轻度润色，共用一把百炼 DashScope API Key。无需注册本应用账号。
      </p>
      <fieldset disabled={busy} className="space-y-4 border-0 p-0 m-0">
        <label className="block space-y-1">
          <span>语音识别</span>
          <select
            className={inputStyle}
            value={config.stt_provider}
            onChange={(e) => update({ stt_provider: e.target.value as typeof config.stt_provider })}
          >
            <option value="aliyun-qwen3-asr">阿里云 Qwen3 实时识别</option>
            {isMacPlatform() && <option value="apple-speech">Apple Speech（系统备用）</option>}
          </select>
        </label>
        <label className="block space-y-1">
          <span>阿里云地域（与密钥一致）</span>
          <select
            className={inputStyle}
            value={config.stt_aliyun_qwen_region}
            onChange={(e) =>
              update({
                stt_aliyun_qwen_region: e.target.value as typeof config.stt_aliyun_qwen_region,
              })
            }
          >
            <option value="china-mainland">中国内地</option>
            <option value="international">国际</option>
          </select>
        </label>
        <label className="block space-y-1">
          <span>DashScope API Key</span>
          <input
            className={inputStyle}
            type="password"
            autoComplete="off"
            spellCheck={false}
            value={key}
            onChange={(e) => setKey(e.target.value)}
            placeholder={
              hasKey ? '已保存；留空保留，输入新密钥可替换' : '在此粘贴密钥，不要发送到聊天中'
            }
          />
        </label>
        <label className="flex gap-2 items-center">
          <input
            type="checkbox"
            checked={config.polish_enabled}
            onChange={(e) => update({ polish_enabled: e.target.checked })}
          />
          启用 Qwen 轻度润色
        </label>
        {config.polish_enabled && (
          <>
            <label className="block space-y-1">
              <span>润色模型</span>
              <input
                className={inputStyle}
                value={config.llm_model}
                onChange={(e) => update({ llm_model: e.target.value })}
              />
            </label>
            <label className="block space-y-1">
              <span>整理程度</span>
              <select
                className={inputStyle}
                value={config.polish_style}
                onChange={(e) =>
                  update({ polish_style: e.target.value as typeof config.polish_style })
                }
              >
                <option value="minimal">只修正口误和标点</option>
                <option value="clean">轻度整理，保留原意</option>
              </select>
            </label>
          </>
        )}
        <div className="flex flex-wrap gap-2">
          <button
            type="button"
            onClick={() => void save(false)}
            className="px-4 py-2 rounded-full bg-accent text-on-accent disabled:opacity-50"
          >
            {busy ? '处理中…' : onSaved ? '保存并继续' : '保存服务配置'}
          </button>
          <button
            type="button"
            onClick={() => void save(true)}
            className="px-4 py-2 rounded-full border border-border"
          >
            保存并测试连接（可能计费）
          </button>
        </div>
      </fieldset>
      {error && (
        <p role="alert" className="text-error">
          {error}
        </p>
      )}
      {message && (
        <p role="status" className="text-success">
          {message}
        </p>
      )}
      <p className="text-text-tertiary text-[12px]">
        选择阿里云时，录音发送至阿里云；启用润色时，转写文字也会发送至阿里云。历史记录仅存本机，默认保留
        30 天。Apple Speech 是否离线取决于系统和语言资源；备用模式需要手动选择。
      </p>
    </div>
  )
}
