import { invoke } from "@tauri-apps/api/core";

export type GpuInfo = {
  has_nvidia: boolean;
  device_name: string | null;
  driver_version: string | null;
  cuda_version: string | null;
};

export type AudioDeviceInfo = {
  name: string;
  is_default: boolean;
  default_sample_rate: number;
  default_channels: number;
};

export type AudioMeter = {
  rms_db: number;
  peak_db: number;
};

export type RecordingStarted = {
  recording_id: string;
  wav_path: string;
};

export type RecordingStopped = {
  wav_path: string;
};

export type WhisperModelState = {
  id: string;
  display_name: string;
  size_bytes: number;
  multilingual: boolean;
  notes: string;
  downloaded: boolean;
  on_disk_bytes: number | null;
  path: string | null;
  imported: boolean;
  /** Note de vitesse de 0 a 1 (0 = inconnu pour les modeles importes). */
  speed: number;
  /** Note de precision de 0 a 1 (0 = inconnu). */
  accuracy: number;
  /** Codes ISO supportes ; "auto" en tete = auto-detect. */
  language_codes: string[];
};

export type DownloadProgress = {
  id: string;
  downloaded: number;
  total: number;
};

export type DownloadComplete = {
  id: string;
  path: string;
};

export type DownloadError = {
  id: string;
  message: string;
};

export type TranscribeRequest = {
  wav_path: string;
  model_id: string;
  language?: string | null;
  initial_prompt?: string | null;
  n_threads?: number | null;
};

export type TranscribeResponse = {
  text: string;
  model_id: string;
  duration_ms: number;
};

export type WordReplacement = {
  id: string;
  original_text: string;
  replacement_text: string;
  date_added: string;
  is_enabled: boolean;
};

export type TranscriptionSource = {
  kind: "local" | "cloud" | "parakeet";
  whisper_model_id: string | null;
  cloud_provider: string | null;
  cloud_model: string | null;
  parakeet_model_id: string | null;
};

export type NewWordReplacement = {
  original_text: string;
  replacement_text: string;
  is_enabled?: boolean;
};

export type UpdateWordReplacement = {
  id: string;
  original_text?: string;
  replacement_text?: string;
  is_enabled?: boolean;
};

export type TextProcessingSettings = {
  text_formatting_enabled: boolean;
  remove_filler_words: boolean;
  filler_words: string[];
  append_trailing_space: boolean;
  restore_clipboard_after_paste: boolean;
};

export type CustomPrompt = {
  id: string;
  title: string;
  prompt_text: string;
  icon: string;
  description: string | null;
  is_predefined: boolean;
  trigger_words: string[];
  use_system_instructions: boolean;
};

export type LLMProviderInfo = {
  id: string;
  label: string;
  endpoint: string;
  default_model: string;
  models: string[];
  requires_api_key: boolean;
  has_api_key: boolean;
};

export type LLMSelection = {
  provider_id: string;
  model: string;
};

export type GgufModelState = {
  id: string;
  display_name: string;
  size_bytes: number;
  context_length: number;
  notes: string;
  downloaded: boolean;
  on_disk_bytes: number | null;
  path: string | null;
  imported: boolean;
};

export type LlamaCppSettings = {
  n_gpu_layers: number;
  context_size: number;
  max_tokens: number;
};

export type PowerAppTrigger = {
  id: string;
  exe_name: string;
  app_name: string;
};

export type PowerUrlTrigger = {
  id: string;
  url: string;
};

export type AutoSendKey = "none" | "enter" | "shift_enter" | "ctrl_enter";

export type PowerModeConfig = {
  id: string;
  name: string;
  emoji: string;
  app_triggers: PowerAppTrigger[];
  url_triggers: PowerUrlTrigger[];
  is_enhancement_enabled: boolean;
  use_screen_capture: boolean | null;
  selected_prompt_id: string | null;
  selected_llm_provider: string | null;
  selected_llm_model: string | null;
  transcription_kind: string | null;
  whisper_model_id: string | null;
  cloud_provider: string | null;
  cloud_model: string | null;
  parakeet_model_id: string | null;
  language: string | null;
  auto_send_key: AutoSendKey;
  is_enabled: boolean;
  is_default: boolean;
};

export type PowerSession = {
  config_id: string;
  config_name: string;
  emoji: string;
};

export type DetectionPreview = {
  active: {
    hwnd: number;
    pid: number;
    title: string;
    exe_name: string;
  };
  url: string | null;
  matched_config_id: string | null;
  matched_config_name: string | null;
};

export type PermissionState = {
  ok: boolean;
  /** i18n key resolved on the frontend (e.g. "permissions.audio.detected"). */
  label_key: string;
  /** Optional interpolation arguments for the label key (e.g. { count: 3 }). */
  label_args?: Record<string, unknown> | null;
  /** Optional i18n key for an extended hint shown when ok = false. */
  hint_key?: string | null;
  /** Raw technical diagnostic (Win32 error message, etc.). Not translated. */
  diagnostic?: string | null;
};

export type PermissionStatus = {
  microphone: PermissionState;
  ocr: PermissionState;
  autostart: PermissionState;
  hotkey: PermissionState;
};

export type TranscriptionRecord = {
  id: string;
  timestamp: string;
  status: "pending" | "completed" | "failed";
  text: string;
  enhanced_text: string | null;
  duration_sec: number | null;
  transcription_duration_sec: number | null;
  enhancement_duration_sec: number | null;
  audio_file_name: string | null;
  transcription_model_name: string | null;
  ai_enhancement_model_name: string | null;
  prompt_name: string | null;
  ai_request_system_message: string | null;
  ai_request_user_message: string | null;
  power_mode_name: string | null;
  power_mode_emoji: string | null;
  language: string | null;
};

export type RetentionSettings = {
  transcription_cleanup: boolean;
  transcription_retention_minutes: number;
  audio_cleanup: boolean;
  audio_retention_days: number;
};

export type ParakeetModelState = {
  id: string;
  display_name: string;
  multilingual: boolean;
  is_quantized: boolean;
  size_bytes: number;
  notes: string;
  downloaded: boolean;
  missing_files: string[];
  on_disk_bytes: number | null;
  path: string | null;
  speed: number;
  accuracy: number;
  language_codes: string[];
};

export const api = {
  ping: () => invoke<string>("ping"),
  getGpuInfo: () => invoke<GpuInfo>("get_gpu_info"),

  listAudioDevices: () => invoke<AudioDeviceInfo[]>("list_audio_devices"),
  startRecording: (deviceName: string | null) =>
    invoke<RecordingStarted>("start_recording", { deviceName }),
  stopRecording: (runPipeline = false) =>
    invoke<RecordingStopped>("stop_recording", { runPipeline }),
  cancelRecording: () => invoke<void>("cancel_recording"),
  getAudioMeter: () => invoke<AudioMeter>("get_audio_meter"),
  isRecording: () => invoke<boolean>("is_recording"),

  listWhisperModels: () => invoke<WhisperModelState[]>("list_whisper_models"),
  downloadWhisperModel: (id: string) => invoke<string>("download_whisper_model", { id }),
  cancelDownloadWhisperModel: (id: string) =>
    invoke<void>("cancel_download_whisper_model", { id }),
  deleteWhisperModel: (id: string) => invoke<void>("delete_whisper_model", { id }),
  importWhisperModel: (path: string) => invoke<string>("import_whisper_model", { path }),

  transcribeWav: (req: TranscribeRequest) =>
    invoke<TranscribeResponse>("transcribe_wav", { req }),

  setSelectedWhisperModel: (id: string | null) =>
    invoke<void>("set_selected_whisper_model", { id }),
  getSelectedWhisperModel: () => invoke<string | null>("get_selected_whisper_model"),

  getDictationLanguage: () => invoke<string>("get_dictation_language"),
  setDictationLanguage: (code: string) =>
    invoke<void>("set_dictation_language", { code }),

  getTextProcessingSettings: () =>
    invoke<TextProcessingSettings>("get_text_processing_settings"),
  setTextFormattingEnabled: (enabled: boolean) =>
    invoke<void>("set_text_formatting_enabled", { enabled }),
  setRemoveFillerWords: (enabled: boolean) =>
    invoke<void>("set_remove_filler_words", { enabled }),
  setFillerWords: (words: string[]) => invoke<void>("set_filler_words", { words }),
  setAppendTrailingSpace: (enabled: boolean) =>
    invoke<void>("set_append_trailing_space", { enabled }),
  setRestoreClipboardAfterPaste: (enabled: boolean) =>
    invoke<void>("set_restore_clipboard_after_paste", { enabled }),
  getCloseToTray: () => invoke<boolean>("get_close_to_tray"),
  setCloseToTray: (enabled: boolean) =>
    invoke<void>("set_close_to_tray", { enabled }),
  getSystemMuteEnabled: () => invoke<boolean>("get_system_mute_enabled"),
  setSystemMuteEnabled: (enabled: boolean) =>
    invoke<void>("set_system_mute_enabled", { enabled }),
  getAudioResumptionDelay: () => invoke<number>("get_audio_resumption_delay"),
  setAudioResumptionDelay: (secs: number) =>
    invoke<void>("set_audio_resumption_delay", { secs }),
  resizeRecorderWindow: (height: number) =>
    invoke<void>("resize_recorder_window", { height }),
  showMainWindow: (panel?: string | null) =>
    invoke<void>("show_main_window", { panel: panel ?? null }),

  listCloudProviders: () =>
    invoke<
      Array<{
        id: string;
        display_name: string;
        requires_api_key: boolean;
        api_key_url: string;
        has_api_key: boolean;
      }>
    >("list_cloud_providers"),
  listCloudModels: () =>
    invoke<
      Array<{
        provider_id: string;
        model_id: string;
        display_name: string;
        supports_batch: boolean;
        supports_streaming: boolean;
        multilingual: boolean;
        notes: string;
        speed: number;
        accuracy: number;
        language_codes: string[];
      }>
    >("list_cloud_models"),
  setApiKey: (provider: string, key: string) =>
    invoke<void>("set_api_key", { provider, key }),
  deleteApiKey: (provider: string) => invoke<void>("delete_api_key", { provider }),
  hasApiKey: (provider: string) => invoke<boolean>("has_api_key", { provider }),
  verifyApiKey: (provider: string, key: string) =>
    invoke<void>("verify_api_key", { provider, key }),
  cloudTranscribeWav: (args: {
    wav_path: string;
    provider: string;
    model: string;
    language?: string | null;
    prompt?: string | null;
    custom_vocabulary?: string[];
  }) =>
    invoke<{
      text: string;
      provider: string;
      model: string;
      duration_ms: number;
    }>("cloud_transcribe_wav", { args }),

  getTranscriptionSource: () =>
    invoke<{
      kind: "local" | "cloud" | "parakeet";
      whisper_model_id: string | null;
      cloud_provider: string | null;
      cloud_model: string | null;
      parakeet_model_id: string | null;
    }>("get_transcription_source"),
  setTranscriptionSource: (source: {
    kind: "local" | "cloud" | "parakeet";
    whisper_model_id?: string | null;
    cloud_provider?: string | null;
    cloud_model?: string | null;
    parakeet_model_id?: string | null;
  }) => invoke<void>("set_transcription_source", { source }),
  setTranscriptionKind: (kind: "local" | "cloud" | "parakeet") =>
    invoke<void>("set_transcription_kind", { kind }),

  vadGetState: () =>
    invoke<{
      downloaded: boolean;
      path: string | null;
      on_disk_bytes: number | null;
    }>("vad_get_state"),
  vadDownload: () => invoke<string>("vad_download"),
  vadDelete: () => invoke<void>("vad_delete"),
  vadIsEnabled: () => invoke<boolean>("vad_is_enabled"),
  vadSetEnabled: (enabled: boolean) => invoke<void>("vad_set_enabled", { enabled }),

  listWordReplacements: () => invoke<WordReplacement[]>("list_word_replacements"),
  addWordReplacement: (payload: NewWordReplacement) =>
    invoke<WordReplacement>("add_word_replacement", { payload }),
  updateWordReplacement: (payload: UpdateWordReplacement) =>
    invoke<WordReplacement>("update_word_replacement", { payload }),
  deleteWordReplacement: (id: string) =>
    invoke<void>("delete_word_replacement", { id }),

  getEnhancementEnabled: () => invoke<boolean>("get_enhancement_enabled"),
  setEnhancementEnabled: (enabled: boolean) =>
    invoke<void>("set_enhancement_enabled", { enabled }),
  listPrompts: () => invoke<CustomPrompt[]>("list_prompts"),
  addPrompt: (prompt: CustomPrompt) =>
    invoke<CustomPrompt>("add_prompt", { prompt }),
  updatePrompt: (prompt: CustomPrompt) =>
    invoke<void>("update_prompt", { prompt }),
  deletePrompt: (id: string) => invoke<void>("delete_prompt", { id }),
  getActivePromptId: () => invoke<string | null>("get_active_prompt_id"),
  setActivePromptId: (id: string | null) =>
    invoke<void>("set_active_prompt_id", { id }),
  listExtraTemplates: () => invoke<CustomPrompt[]>("list_extra_templates"),
  listLlmProviders: () => invoke<LLMProviderInfo[]>("list_llm_providers"),
  getLlmSelection: () => invoke<LLMSelection | null>("get_llm_selection"),
  setLlmSelection: (providerId: string, model: string) =>
    invoke<void>("set_llm_selection", { providerId, model }),

  getOllamaBaseUrl: () => invoke<string>("get_ollama_base_url"),
  setOllamaBaseUrl: (url: string) =>
    invoke<void>("set_ollama_base_url", { url }),
  listOllamaModels: () => invoke<string[]>("list_ollama_models"),
  getCustomBaseUrl: () => invoke<string | null>("get_custom_base_url"),
  setCustomBaseUrl: (url: string) =>
    invoke<void>("set_custom_base_url", { url }),

  getLocalcliCustomCmd: () =>
    invoke<string | null>("get_localcli_custom_cmd"),
  setLocalcliCustomCmd: (cmd: string) =>
    invoke<void>("set_localcli_custom_cmd", { cmd }),
  getLocalcliTimeoutSecs: () =>
    invoke<number>("get_localcli_timeout_secs"),
  setLocalcliTimeoutSecs: (secs: number) =>
    invoke<void>("set_localcli_timeout_secs", { secs }),

  listGgufModels: () => invoke<GgufModelState[]>("list_gguf_models"),
  downloadGgufModel: (id: string) =>
    invoke<string>("download_gguf_model", { id }),
  cancelDownloadGgufModel: (id: string) =>
    invoke<void>("cancel_download_gguf_model", { id }),
  deleteGgufModel: (id: string) =>
    invoke<void>("delete_gguf_model", { id }),
  importGgufModel: () => invoke<string>("import_gguf_model"),
  getSelectedGguf: () => invoke<string | null>("get_selected_gguf"),
  setSelectedGguf: (id: string | null) =>
    invoke<void>("set_selected_gguf", { id }),
  getLlamacppSettings: () =>
    invoke<LlamaCppSettings>("get_llamacpp_settings"),
  setLlamacppSettings: (args: LlamaCppSettings) =>
    invoke<void>("set_llamacpp_settings", args),
  llamacppCudaEnabled: () => invoke<boolean>("llamacpp_cuda_enabled"),

  listParakeetModels: () =>
    invoke<ParakeetModelState[]>("list_parakeet_models"),
  downloadParakeetModel: (id: string) =>
    invoke<string>("download_parakeet_model", { id }),
  cancelDownloadParakeetModel: (id: string) =>
    invoke<void>("cancel_download_parakeet_model", { id }),
  deleteParakeetModel: (id: string) =>
    invoke<void>("delete_parakeet_model", { id }),
  parakeetExecutionProvider: () =>
    invoke<string>("parakeet_execution_provider"),

  listPowerConfigs: () => invoke<PowerModeConfig[]>("list_power_configs"),
  addPowerConfig: (config: PowerModeConfig) =>
    invoke<PowerModeConfig>("add_power_config", { config }),
  updatePowerConfig: (config: PowerModeConfig) =>
    invoke<void>("update_power_config", { config }),
  deletePowerConfig: (id: string) =>
    invoke<void>("delete_power_config", { id }),
  reorderPowerConfigs: (orderedIds: string[]) =>
    invoke<void>("reorder_power_configs", { orderedIds }),
  getPowerAutoRestore: () => invoke<boolean>("get_power_auto_restore"),
  setPowerAutoRestore: (enabled: boolean) =>
    invoke<void>("set_power_auto_restore", { enabled }),
  getActivePowerSession: () =>
    invoke<PowerSession | null>("get_active_power_session"),
  powerModePreview: () => invoke<DetectionPreview>("power_mode_preview"),

  getScreenContextEnabled: () =>
    invoke<boolean>("get_screen_context_enabled"),
  setScreenContextEnabled: (enabled: boolean) =>
    invoke<void>("set_screen_context_enabled", { enabled }),
  getScreenContextCached: () =>
    invoke<string | null>("get_screen_context_cached"),
  clearScreenContext: () => invoke<void>("clear_screen_context"),
  captureScreenContextPreview: () =>
    invoke<string | null>("capture_screen_context_preview"),

  listHistory: (args?: {
    limit?: number;
    before?: string | null;
    search?: string | null;
  }) =>
    invoke<TranscriptionRecord[]>("list_history", {
      limit: args?.limit ?? null,
      before: args?.before ?? null,
      search: args?.search ?? null,
    }),
  getHistoryItem: (id: string) =>
    invoke<TranscriptionRecord | null>("get_history_item", { id }),
  deleteHistoryItem: (id: string) =>
    invoke<void>("delete_history_item", { id }),
  countHistory: () => invoke<number>("count_history"),
  exportHistoryCsv: (ids: string[]) =>
    invoke<string | null>("export_history_csv", { ids }),
  getRetentionSettings: () =>
    invoke<RetentionSettings>("get_retention_settings"),
  setRetentionSettings: (settings: RetentionSettings) =>
    invoke<void>("set_retention_settings", { settings }),
  runHistoryCleanup: () => invoke<void>("run_history_cleanup"),

  checkPermissions: () => invoke<PermissionStatus>("check_permissions"),
  setAutostartEnabled: (enabled: boolean) =>
    invoke<void>("set_autostart_enabled", { enabled }),
  openPrivacyMicrophone: () => invoke<void>("open_privacy_microphone"),
  openLanguageSettings: () => invoke<void>("open_language_settings"),

  getRecorderStyle: () => invoke<string>("get_recorder_style"),
  setRecorderStyle: (style: "mini" | "notch") =>
    invoke<void>("set_recorder_style", { style }),

  getOnboardingCompleted: () => invoke<boolean>("get_onboarding_completed"),
  setOnboardingCompleted: (completed: boolean) =>
    invoke<void>("set_onboarding_completed", { completed }),

  getHotkeyConfig: () => invoke<HotkeyConfig>("get_hotkey_config"),
  setHotkeyConfig: (config: HotkeyConfig) =>
    invoke<void>("set_hotkey_config", { config }),
  resetHotkeyConfig: () => invoke<HotkeyConfig>("reset_hotkey_config"),
  listHotkeyOptions: () => invoke<HotkeyOptionInfo[]>("list_hotkey_options"),
};

// -- Hotkey types ----------------------------------------------------------
//
// Mappent les structs Rust dans src-tauri/src/commands/hotkey.rs et
// src-tauri/src/hotkeys/keyboard_hook.rs. Le tag `kind` differencie les
// trois variantes possibles.

export type HotkeyOptionId =
  | "none"
  | "rightAlt"
  | "leftAlt"
  | "leftCtrl"
  | "rightCtrl"
  | "rightWin"
  | "rightShift"
  | "leftShift";

export type HotkeyTrigger =
  | { kind: "none" }
  | { kind: "modifier"; option: HotkeyOptionId }
  | {
      kind: "combo";
      vk: number;
      ctrl: boolean;
      alt: boolean;
      shift: boolean;
      win: boolean;
    };

export type HotkeyMode = "toggle" | "pushToTalk" | "hybrid";

export type HotkeySlotConfig = {
  trigger: HotkeyTrigger;
  mode: HotkeyMode;
};

export type HotkeyConfig = {
  primary: HotkeySlotConfig;
  secondary: HotkeySlotConfig;
};

export type HotkeyOptionInfo = {
  id: HotkeyOptionId;
  vk: number | null;
};
