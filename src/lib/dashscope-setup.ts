import type { AppConfig } from '../stores/appStore'
import { getConfig, getCredentialStatus, setCredential, updateConfig } from './tauri'

export async function saveDashscopeSetup(config: AppConfig, typedKey: string): Promise<AppConfig> {
  const key = typedKey.trim()
  if (key && (key.length > 4096 || /\s/.test(key))) throw new Error('请填写有效的 API Key。')
  if (config.stt_provider !== 'aliyun-qwen3-asr' && config.stt_provider !== 'apple-speech') {
    throw new Error('请选择阿里云或 Apple Speech。')
  }
  if (config.polish_enabled && !config.llm_model.trim()) throw new Error('请填写润色模型名称。')
  try {
    if (key) await setCredential('stt', 'aliyun-qwen3-asr', key)
    if (config.stt_provider === 'aliyun-qwen3-asr' || config.polish_enabled) {
      const status = await getCredentialStatus('stt', 'aliyun-qwen3-asr')
      if (!status.hasSecret || status.storage !== 'os-vault') throw new Error('missing credential')
    }
  } catch {
    // Never display vault errors that could contain a user credential.
    throw new Error('密钥未保存。请填写 DashScope API Key，并允许访问系统钥匙串后重试。')
  }
  const baseUrl =
    config.stt_aliyun_qwen_region === 'international'
      ? 'https://dashscope-intl.aliyuncs.com/compatible-mode/v1'
      : 'https://dashscope.aliyuncs.com/compatible-mode/v1'
  try {
    await updateConfig({
      ...config,
      llm_provider: 'qwen',
      llm_base_url: baseUrl,
      llm_model: config.llm_model.trim(),
      stt_api_key: '',
      stt_custom_api_key: '',
      llm_api_key: '',
    })
    return await getConfig()
  } catch {
    throw new Error('设置未保存，请重试。已写入的密钥保留在系统钥匙串中。')
  }
}
