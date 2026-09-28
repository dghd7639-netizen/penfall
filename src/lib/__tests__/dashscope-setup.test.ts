import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useAppStore } from '../../stores/appStore'
import { saveDashscopeSetup } from '../dashscope-setup'
import { getConfig, getCredentialStatus, setCredential, updateConfig } from '../tauri'
vi.mock('../tauri', () => ({
  getConfig: vi.fn(),
  getCredentialStatus: vi.fn(),
  setCredential: vi.fn(),
  updateConfig: vi.fn(),
}))
const config = () => ({ ...useAppStore.getInitialState().config })
beforeEach(() => {
  vi.resetAllMocks()
  vi.mocked(getCredentialStatus).mockResolvedValue({
    namespace: 'stt',
    provider: 'aliyun-qwen3-asr',
    hasSecret: true,
    storage: 'os-vault',
    updatedAt: null,
  })
  vi.mocked(getConfig).mockResolvedValue(config())
})
describe('single DashScope key setup', () => {
  it('saves one vault entry and never includes a key in app config', async () => {
    await saveDashscopeSetup(
      { ...config(), stt_aliyun_qwen_region: 'international' },
      '  test-key  ',
    )
    expect(setCredential).toHaveBeenCalledExactlyOnceWith('stt', 'aliyun-qwen3-asr', 'test-key')
    expect(updateConfig).toHaveBeenCalledWith(
      expect.objectContaining({
        llm_provider: 'qwen',
        llm_base_url: 'https://dashscope-intl.aliyuncs.com/compatible-mode/v1',
        stt_api_key: '',
        llm_api_key: '',
      }),
    )
    expect(JSON.stringify(vi.mocked(updateConfig).mock.calls)).not.toContain('test-key')
  })
  it('keeps saved credentials when the password field is blank', async () => {
    await saveDashscopeSetup(config(), '')
    expect(setCredential).not.toHaveBeenCalled()
    expect(updateConfig).toHaveBeenCalledOnce()
  })
  it('fails closed on vault failure without echoing the key or saving config', async () => {
    vi.mocked(setCredential).mockRejectedValue(new Error('secret-test-key'))
    const result = saveDashscopeSetup(config(), 'secret-test-key')
    await expect(result).rejects.toThrow('密钥未保存')
    await expect(result).rejects.not.toThrow('secret-test-key')
    expect(updateConfig).not.toHaveBeenCalled()
  })
  it('allows Apple Speech without a key when polish is disabled', async () => {
    await saveDashscopeSetup(
      { ...config(), stt_provider: 'apple-speech', polish_enabled: false },
      '',
    )
    expect(getCredentialStatus).not.toHaveBeenCalled()
    expect(updateConfig).toHaveBeenCalledOnce()
  })
  it('does not complete when persistence fails', async () => {
    vi.mocked(updateConfig).mockRejectedValue(new Error('disk unavailable'))
    await expect(saveDashscopeSetup(config(), '')).rejects.toThrow('设置未保存')
    expect(getConfig).not.toHaveBeenCalled()
  })
})
