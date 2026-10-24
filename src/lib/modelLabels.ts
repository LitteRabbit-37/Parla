// Model metadata comes from Rust in French. Resolve built-in catalog text at
// render time, like promptLabels, so switching the UI language updates it.
// IDs, provider names, and user-chosen names remain the backend's identifiers.
import type { TFunction } from "i18next";

type ModelText = { display_name: string; notes: string };
type LocalModel = ModelText & { id: string; imported?: boolean };
type CloudModel = ModelText & { provider_id: string; model_id: string };
type LocalKind = "whisper" | "parakeet" | "llm";

const CATALOG_KEYS: Record<LocalKind | "cloud", Record<string, string>> = {
  whisper: {
    "ggml-tiny": "tiny",
    "ggml-tiny.en": "tinyEn",
    "ggml-base": "base",
    "ggml-base.en": "baseEn",
    "ggml-large-v2": "largeV2",
    "ggml-large-v3": "largeV3",
    "ggml-large-v3-turbo": "largeV3Turbo",
    "ggml-large-v3-turbo-q5_0": "largeV3TurboQ5",
  },
  parakeet: {
    "parakeet-tdt-0.6b-v2": "v2",
    "parakeet-tdt-0.6b-v2-int8": "v2Int8",
    "parakeet-tdt-0.6b-v3": "v3",
    "parakeet-tdt-0.6b-v3-int8": "v3Int8",
  },
  cloud: {
    "groq/whisper-large-v3-turbo": "groqWhisperTurbo",
    "elevenlabs/scribe_v1": "scribeV1",
    "elevenlabs/scribe_v2_medical": "scribeMedical",
    "elevenlabs/scribe_v2": "scribeRealtime",
    "deepgram/nova-3": "nova3",
    "deepgram/nova-3-medical": "nova3Medical",
    "mistral/voxtral-mini-latest": "voxtralMini",
    "mistral/voxtral-mini-transcribe-realtime-2602": "voxtralRealtime",
    "soniox/stt-async-v5": "sonioxAsync",
    "soniox/stt-rt-v5": "sonioxRealtime",
    "speechmatics/speechmatics-enhanced": "speechmatics",
    "gemini/gemini-2.5-pro": "geminiPro",
    "gemini/gemini-2.5-flash": "geminiFlash",
    "xai/grok-voice-transcribe-2.0": "grokVoice",
    "cartesia/ink-2": "ink2",
    "assemblyai/universal-3-6-pro": "universal36",
    "assemblyai/universal-3-5-pro": "universal35",
    "assemblyai/universal-2": "universal2",
  },
  llm: {
    "qwen2.5-3b-instruct-q4": "qwen25",
    "llama-3.2-3b-instruct-q4": "llama32",
    "gemma-2-2b-it-q4": "gemma2",
    "phi-3.5-mini-instruct-q4": "phi35",
  },
};

function catalogText(
  t: TFunction,
  kind: LocalKind | "cloud",
  id: string,
  model: ModelText,
): { name: string; notes: string } {
  const keys = CATALOG_KEYS[kind];
  if (!Object.prototype.hasOwnProperty.call(keys, id)) {
    return { name: model.display_name, notes: model.notes };
  }
  const prefix = `modelCatalog.${kind}.${keys[id]}`;
  return {
    name: kind === "whisper" || kind === "parakeet"
      ? t(`${prefix}.name`, { defaultValue: model.display_name })
      : model.display_name,
    notes: t(`${prefix}.notes`, { defaultValue: model.notes }),
  };
}

export function localModelText(t: TFunction, kind: LocalKind, model: LocalModel) {
  // Imported models have user-chosen names but a standard backend description.
  if (model.imported && (kind === "whisper" || kind === "llm")) {
    return {
      name: model.display_name,
      notes: t(`modelCatalog.imported.${kind}`),
    };
  }
  return catalogText(t, kind, model.id, model);
}

export function cloudModelText(t: TFunction, model: CloudModel) {
  // Model IDs are only unique within their provider.
  return catalogText(t, "cloud", `${model.provider_id}/${model.model_id}`, model);
}

// Decimal units match the catalog's byte counts; Intl localizes both the
// number and the unit (e.g. GB in German, Go in French).
export function formatModelBytes(bytes: number | null | undefined, locale: string) {
  if (bytes == null || !Number.isFinite(bytes) || bytes < 0) return "-";
  const [divisor, unit]: [number, string] = bytes >= 1e9
    ? [1e9, "gigabyte"]
    : bytes >= 1e6
      ? [1e6, "megabyte"]
      : bytes >= 1e3
        ? [1e3, "kilobyte"]
        : [1, "byte"];
  return new Intl.NumberFormat(locale, {
    style: "unit",
    unit,
    unitDisplay: "short",
    maximumFractionDigits: 1,
  }).format(bytes / divisor);
}
