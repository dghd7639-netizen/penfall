# Cloud STT Error and Capsule Visibility Fix Design

## Background

Issue #46 was closed after fixing two crash/freeze paths:

- clicking the recording capsule cancel button no longer bubbles into the outer capsule stop handler;
- Cloud STT buffering no longer busy-spins while waiting for upload-based transcription.

A follow-up manual test exposed a remaining user-facing failure mode. A Cloud STT request returned HTTP 403 from `/api/proxy/stt`, but the desktop capsule later showed `No speech detected. Please try again.` This hides the real reason and makes a quota/auth failure look like a microphone or silence problem.

The desktop capsule also has an existing `capsule_auto_hide` setting, but the right-click capsule menu does not expose it. Users see a permanent desktop microphone icon unless they discover the setting page.

## Current Evidence

- Cloud STT provider maps every HTTP 403 from the cloud STT endpoint to `AppError::Auth`.
- `AppError::Auth` maps to the frontend error code `stt_invalid_key`.
- The STT task can emit an STT error, then `wait_for_stt()` sees an empty accumulated transcript and emits the plain string `No speech detected. Please try again.`
- The frontend already supports structured `pipeline:error` payloads shaped as `{ code, details, retry_count }`, but locale files currently do not define the STT error codes under `errors`.
- `capsule_auto_hide` already exists in Rust storage and frontend state.
- `useCapsuleResize()` already hides the capsule while idle and shows it when leaving idle, but the behavior is settings-only and does not explicitly treat an error as a visible state.
- `CapsuleRecording` already shows volume-driven waveform bars through `audioVolume`.
- `DurationTimer` already shows recording duration and auto-stops at the configured maximum.
- `CapsuleProcessing` and `CapsulePolishing` already represent transcribing/thinking states.

## Goals

1. Preserve the real cloud STT failure reason in the desktop UI.
2. Distinguish Cloud STT quota exhaustion from invalid auth/session errors.
3. Make no-speech a structured, localized error shown only when STT actually succeeds with empty text.
4. Add complete locale coverage for the new and existing STT error codes.
5. Let users hide the idle desktop capsule from the capsule right-click menu.
6. Keep the capsule visible during recording, transcribing, polishing, outputting, and short-lived error display.
7. Give users a recovery path when the idle capsule is hidden and the right-click menu is inaccessible.

## Non-Goals

- Do not redesign the cloud quota policy or change server-side quota reservation amounts in this desktop fix.
- Do not replace the existing waveform, timer, transcribing, polishing, or outputting capsule components.
- Do not remove the system tray icon; it remains the always-available access point for settings, history, recording, and quit.
- Do not force existing users who deliberately keep the capsule visible into auto-hide without a clear persisted setting change.

## Recommended Approach

Reuse the existing `capsule_auto_hide` configuration and make it visible, recoverable, and immediate.

This is better than inventing a second "hide desktop icon" setting because the code already has the right behavior model: hide only while idle, show during active states. The fix should harden that model and expose it where users naturally look.

Rejected alternatives:

- Always hide the capsule while idle with no setting. This removes user control and makes the feature harder to discover.
- Keep the setting only in the settings page. This preserves the current poor discoverability.
- Create a separate "desktop icon hidden" flag. This duplicates `capsule_auto_hide` and risks conflicting states.

## Desired Behavior

### Cloud STT Errors

When Cloud STT receives HTTP 403:

- If the response error message indicates quota exhaustion, desktop emits `stt_quota_exceeded`.
- Otherwise desktop emits `stt_invalid_key`.
- The frontend renders the localized `errors.<code>` message.
- The pipeline does not emit `stt_no_speech_detected` afterward for the same recording session.

When Cloud STT succeeds but returns empty text:

- Desktop emits `stt_no_speech_detected`.
- The error is localized.
- This message is reserved for real silence or unintelligible audio, not quota/auth/network failures.

### Capsule Visibility

When `capsule_auto_hide` is enabled:

- idle with no error: capsule window is hidden;
- recording: capsule window shows waveform, recording duration, and cancel action;
- transcribing: capsule window shows transcribing state and partial transcript when available;
- polishing: capsule window shows thinking state;
- outputting: capsule window shows completion state briefly;
- error: capsule window shows the localized error briefly, then returns to idle hidden state.

When `capsule_auto_hide` is disabled:

- idle capsule remains visible as the small microphone capsule.

### Menus and Recovery

Capsule right-click menu:

- shows `Hide capsule when idle` when `capsule_auto_hide` is disabled;
- shows `Keep capsule visible` when `capsule_auto_hide` is enabled;
- toggles the setting immediately;
- persists the new config immediately through the Tauri `update_config` command;
- hides the capsule immediately if toggled on while idle and no error is active;
- shows the capsule immediately if toggled off while idle.

System tray menu:

- adds the same capsule visibility toggle as a recovery path;
- remains available even when the desktop capsule is hidden.

Settings page:

- keeps the existing `Hide capsule when idle` toggle;
- should stay in sync after right-click or tray toggles.

Default behavior:

- new installs should default `capsule_auto_hide` to `true`;
- existing stored configs should keep their persisted preference.

## Architecture

### Rust Error Model

Add an explicit quota error variant, for example:

- `AppError::Quota(String)`

Mapping:

- `Quota` is not retryable.
- `Quota` maps to `UserError { code: "stt_quota_exceeded", details, retry_count: 0 }`.
- 401 and non-quota 403 continue to map to `stt_invalid_key`.

Cloud STT should parse the HTTP 403 body and classify quota-like responses by stable server code when available, with a conservative fallback to message matching for current server responses such as `quota exceeded` or `BYOK`.

### Pipeline Error Latching

Track the last STT failure for the active session. Once an STT provider error is emitted, `wait_for_stt()` must not emit no-speech for that same session.

This can be done with a small `Arc<Mutex<Option<UserError>>>` field on `PipelineHandle`, cleared at the start of each recording and set by STT error branches.

### Frontend i18n

Add an `errors` object to every locale file if it is missing. Include at least:

- `stt_timeout`
- `stt_invalid_key`
- `stt_failed`
- `stt_quota_exceeded`
- `stt_no_speech_detected`
- `output_fallback_clipboard`

The frontend should continue to support plain string errors temporarily for compatibility, but new backend STT errors should use structured payloads.

### Capsule Visibility UI

Add a menu item to `CapsuleContextMenu` that reads `config.capsule_auto_hide`, toggles it, and calls the persisted `updateConfig()` API with the full config object.

Add or expose a Tauri command for toggling `capsule_auto_hide` from Rust tray menu events. The command should:

- load config;
- set `capsule_auto_hide`;
- save config;
- emit a frontend event so open webviews update their Zustand state;
- refresh tray labels.

Prefer a single helper for saving capsule visibility so the capsule menu and tray menu cannot diverge.

## Implementation Notes

- `useCapsuleResize()` should treat `hasError` as an active visible state under auto-hide.
- The context menu itself forces the capsule window visible while open.
- The right-click menu should close after toggling visibility.
- Use lucide `EyeOff` for hiding and `Eye` for keeping visible.
- Menu text must be localized under `capsule.menu`.
- Rust tray labels must include the same concept in all supported UI languages.
- The settings dirty bar may still be used for ordinary settings page edits; the capsule menu and tray menu are immediate actions.

## Test Plan

Rust:

- `AppError::Quota` is not retryable.
- `AppError::Quota` maps to `stt_quota_exceeded`.
- Cloud STT 403 quota body maps to `AppError::Quota`.
- Cloud STT 403 non-quota body maps to `AppError::Auth`.
- A latched STT error prevents no-speech emission for the same session.
- Real empty successful STT can still emit `stt_no_speech_detected`.
- Tray menu builds the capsule visibility item for enabled and disabled states.

Frontend:

- Every locale defines all required `errors` keys.
- Capsule right-click menu shows `Hide capsule when idle` when auto-hide is off.
- Capsule right-click menu shows `Keep capsule visible` when auto-hide is on.
- Toggling from the capsule menu updates local config and persists through `updateConfig`.
- Auto-hide sizing/show logic treats error as visible.
- Existing recording cancel button tests continue to pass.

Manual:

- With Cloud free quota exhausted, recording shows a localized quota message, not no-speech.
- With a real silent recording and available quota, recording shows localized no-speech.
- With auto-hide on, idle capsule disappears; hotkey recording makes it reappear with waveform and timer.
- Right-click menu can enable auto-hide.
- System tray can restore a hidden idle capsule.

## Release and Communication

Release notes should describe this as a desktop UX/error hardening fix:

- Cloud STT quota/auth errors now show the correct localized message.
- Empty speech is no longer used as a fallback message for cloud failures.
- The desktop capsule can be hidden while idle from the right-click menu and restored from the tray/settings.

## Spec Review

Findings:

- P1: A right-click-only hide toggle is not recoverable after the capsule hides. The spec fixes this by requiring the same toggle in the system tray and preserving the settings page toggle.
- P1: Auto-hide could hide error messages if visibility is keyed only on `pipelineState !== "idle"`. The spec fixes this by treating `hasError` as an active visible state.
- P2: Adding new structured backend error codes without locale tests would repeat the current missing-i18n risk. The spec requires locale coverage tests.
- P2: Changing the default to hidden may surprise existing users. The spec limits the default change to new installs and preserves stored preferences.

Self-review result:

- No placeholders remain.
- The cloud error and capsule visibility requirements are separated but share one release scope.
- The design keeps existing waveform/timer/status work instead of rebuilding it.
- The implementation scope is suitable for one TDD plan.
