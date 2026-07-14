// Types et helpers partages de la page AI Models (refonte VoiceInk).
//
// Reference VoiceInk : ModelManagementView.swift. Le modele est l'unite de
// base : le filtre (Recommended / Local / Cloud) est purement visuel et
// l'activation passe toujours par "Definir par defaut" sur une card.

import type {
  ParakeetModelState,
  TranscriptionSource,
  WhisperModelState,
} from "@/lib/tauri";

/// Miroir de commands::cloud CloudProviderInfo (list_cloud_providers).
export type CloudProvider = {
  id: string;
  display_name: string;
  requires_api_key: boolean;
  api_key_url: string;
  has_api_key: boolean;
};

/// Miroir de commands::cloud CloudModelInfo (list_cloud_models).
export type CloudModel = {
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
};

/// Payload des events parakeet_model:download:progress.
export type ParakeetDownloadProgress = {
  id: string;
  downloaded: number;
  total: number;
  current_file: string;
};

export type ModelFilter = "recommended" | "local" | "cloud";

/// Row unifiee de la liste de modeles (dispatch par card).
export type ModelRow =
  | { type: "whisper"; key: string; model: WhisperModelState }
  | { type: "parakeet"; key: string; model: ParakeetModelState }
  | { type: "cloud"; key: string; model: CloudModel };

/// Modeles recommandes, memes ids et meme ordre que VoiceInk
/// ModelManagementView.filteredModels (.recommended). Le 4e est le modele
/// cloud Groq (model_id, unique dans le catalogue).
export const RECOMMENDED_MODELS = [
  "ggml-base.en",
  "parakeet-tdt-0.6b-v2",
  "ggml-large-v3-turbo-q5_0",
  "whisper-large-v3-turbo",
];

/// Nom canonique d'une row pour le matching Recommended (id local,
/// model_id cloud - equivalent du `model.name` VoiceInk).
export function rowName(row: ModelRow): string {
  return row.type === "cloud" ? row.model.model_id : row.model.id;
}

/// Equivalent du `isCurrent` VoiceInk : la row est-elle le modele que le
/// pipeline utilisera a l'enregistrement ?
export function isRowCurrent(
  row: ModelRow,
  source: TranscriptionSource | null,
): boolean {
  if (!source) return false;
  switch (row.type) {
    case "whisper":
      return source.kind === "local" && source.whisper_model_id === row.model.id;
    case "parakeet":
      return (
        source.kind === "parakeet" &&
        source.parakeet_model_id === row.model.id
      );
    case "cloud":
      return (
        source.kind === "cloud" &&
        source.cloud_provider === row.model.provider_id &&
        source.cloud_model === row.model.model_id
      );
  }
}

/// DisplayName du modele actif pour le header "Default Model"
/// (equivalent VoiceInk currentTranscriptionModel?.displayName).
export function resolveDefaultDisplayName(
  source: TranscriptionSource | null,
  whisper: WhisperModelState[],
  parakeet: ParakeetModelState[],
  cloudModels: CloudModel[],
): string | null {
  if (!source) return null;
  switch (source.kind) {
    case "local":
      return (
        whisper.find((m) => m.id === source.whisper_model_id)?.display_name ??
        null
      );
    case "parakeet":
      return (
        parakeet.find((m) => m.id === source.parakeet_model_id)
          ?.display_name ?? null
      );
    case "cloud":
      return (
        cloudModels.find(
          (m) =>
            m.provider_id === source.cloud_provider &&
            m.model_id === source.cloud_model,
        )?.display_name ?? null
      );
    default:
      return null;
  }
}
