<p align="center">
  <img src="src-tauri/icons/128x128@2x.png" width="112" height="112" alt="Penfall" />
</p>

<h1 align="center">Penfall</h1>

<p align="center">话音落下，笔迹成字。<br />一个 macOS 中文语音输入工具。</p>

---

按一下快捷键说话，再按一下结束：语音被转写、轻度整理，然后直接输入到当前光标所在的位置。适用于 AI 对话、微信、浏览器搜索框、文档和邮件。消息不会自动发送。

## 特点

- **中文优先**：默认用阿里云 Qwen 实时识别转写；识别服务也可以换成 Deepgram、AssemblyAI、火山引擎、Whisper 兼容接口，或 macOS 自带的 Apple Speech。
- **轻度润色**：清理口水字、卡壳和重复，补全标点，保留原意。润色用的大模型可以独立选择，支持任何 OpenAI 兼容接口（包括本机网关）。
- **短句直接输出**：没有口水字、卡壳和改口的短句不送去润色，直接输入识别结果，省去等待；本地判断，不调用模型。
- **转不出来也不丢字**：润色失败、没能输入到原来的位置或识别中途出错时，文字会留在一个可复制的悬浮条里，同时存进历史。
- **自动学习纠错**：在输入框里改了字，停下 3 秒就自动记进词典，下次直接纠正；每条规则单独提示、可撤销。
- **多种触发方式**：Fn（按住或切换）、自定义快捷键，还有鼠标中键 / 侧键。
- **提示音可换**：四套原创提示音，也可以关闭。
- **你的数据留在本机**：API Key 只存系统钥匙串；历史记录只存本机，默认保留 30 天，可以关闭和清空。

## 构建

需要 macOS、Node 20 以上、Rust 1.82 以上和 CMake。

```bash
npm install
npx tauri build --debug --bundles app
```

构建出的 `Penfall.app` 在 `src-tauri/target/debug/bundle/macos/`。

第一次运行需要在 系统设置 → 隐私与安全性 里给它 **麦克风** 和 **辅助功能** 权限（用来录音、注册 Fn 快捷键和把文字输入到其它应用）。

- **签名**：macOS 按签名记住这些权限。每次重建都用同一个签名身份签名，权限就不会失效：`APPLE_SIGNING_IDENTITY="<你的签名身份>" npx tauri build --debug --bundles app`。
- **启动方式**：把 `Penfall.app` 复制到 `/Applications` 后用 `open` 或启动台打开。不要在终端里直接运行 `Contents/MacOS/Penfall`，否则辅助功能权限会算到终端头上，快捷键会注册失败。
- **看日志**：`open -n --env RUST_LOG=debug --stdout ~/Library/Logs/penfall.log --stderr ~/Library/Logs/penfall.log /Applications/Penfall.app`。
- **自己用的话**：建议把 `src-tauri/tauri.conf.json` 里的 `identifier` 改成你自己的包标识。

## 配置

- **识别**：设置 →「语音与润色」选识别服务。默认的阿里云 Qwen 识别需要 DashScope API Key，中国内地和国际地域都支持。
- **润色**：同一页选润色服务、地址和模型，填好后点「连接测试」确认能连通。建议用不带推理的轻量模型（例如阿里云 `qwen-flash`）；先推理再回答的模型常常要几十秒。
- **快捷键**：设置 →「通用」里改听写快捷键（默认按住 Fn 说话）、鼠标按键和提示音。

## 隐私

- 录音发送给你选择的识别服务；启用润色时，转写出的文字会发送给你选择的大模型服务。
- 本机历史不做云备份。
- 日志只记录事件类型、字数和耗时，不记录你说的内容。

## 致谢与许可

Penfall 基于 [OpenTypeless](https://github.com/tover0314-w/opentypeless) 二次开发，遵循 MIT 许可。上游版权声明保留在 [LICENSE](LICENSE)，其他第三方组件见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。提示音由 `scripts/generate-cues.py` 和 `scripts/generate-cue-candidates.py` 合成，不含第三方音频。
