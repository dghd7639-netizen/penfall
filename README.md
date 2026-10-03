<p align="center">
  <img src="src-tauri/icons/128x128@2x.png" width="112" height="112" alt="Penfall" />
</p>

<h1 align="center">Penfall</h1>

<p align="center">话音落下，笔迹成字。<br />一个自用的 macOS 中文语音输入工具。</p>

---

按一下快捷键说话，再按一下结束：语音被转写、轻度整理，然后直接输入到当前光标所在的位置。适用于 AI 对话、微信、浏览器搜索框、文档和邮件。消息不会自动发送。

## 特点

- **中文优先**：默认用阿里云 Qwen 实时识别转写；识别服务也可以换成 Deepgram、AssemblyAI、火山引擎、Whisper 兼容接口，或 macOS 自带的 Apple Speech。
- **轻度润色**：清理口水字、卡壳和重复，补全标点，保留原意。润色用的大模型可以独立选择，支持任何 OpenAI 兼容接口（包括本机网关）。
- **自动学习纠错**：在输入框里改了字，停下 3 秒就自动记进词典，下次直接纠正；每条规则单独提示、可撤销。
- **多种触发方式**：Fn（按住或切换）、自定义快捷键，还有鼠标中键 / 侧键。
- **提示音可换**：四套原创提示音，也可以关闭。
- **你的数据留在本机**：API Key 只存系统钥匙串；历史记录只存本机，默认保留 30 天，可以关闭和清空。

## 构建

需要 Node 20 以上、Rust 1.82 以上和 CMake。

```bash
npm install
npx tauri build --debug --bundles app
```

构建出的 `Penfall.app` 在 `src-tauri/target/debug/bundle/macos/`。在 macOS 上要用同一个本地签名身份重复签名，系统权限（麦克风、辅助功能）才不会在每次重建后失效。构建、签名、诊断的细节见 [docs/personal-mvp.md](docs/personal-mvp.md)。

## 隐私

- 录音发送给你选择的识别服务；启用润色时，转写出的文字会发送给你选择的大模型服务。
- 本机历史不做云备份。
- 日志只记录事件类型、字数和耗时，不记录你说的内容。

## 致谢与许可

Penfall 基于 [OpenTypeless](https://github.com/tover0314-w/opentypeless) 二次开发，遵循 MIT 许可。上游版权声明保留在 [LICENSE](LICENSE)，其他第三方组件见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。提示音由 `scripts/generate-cues.py` 和 `scripts/generate-cue-candidates.py` 合成，不含第三方音频。
