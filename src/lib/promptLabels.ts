// Helpers pour traduire les noms et descriptions des prompts predefined.
//
// Les prompts predefined (seedes par le backend a l'init) sont identifies
// par des UUID stables (cf src-tauri/src/enhancement/prompts.rs ID_DEFAULT,
// ID_ASSISTANT). Leur titre + description sont stockes en clair dans
// parla.prompts.json et donc figes pour l'utilisateur. Pour les afficher
// traduits dans l'UI sans casser la persistance, on remappe par ID au
// moment du rendu via i18next.
//
// Les prompts custom de l'utilisateur (`is_predefined: false`) gardent leurs
// titre/description tels quels.

import type { TFunction } from "i18next";
import type { CustomPrompt } from "./tauri";

export const PREDEFINED_PROMPT_IDS = {
  default: "00000000-0000-0000-0000-000000000001",
  assistant: "00000000-0000-0000-0000-000000000002",
} as const;

const ID_TO_KEY: Record<string, { title: string; description: string }> = {
  [PREDEFINED_PROMPT_IDS.default]: {
    title: "prompts.default.title",
    description: "prompts.default.description",
  },
  [PREDEFINED_PROMPT_IDS.assistant]: {
    title: "prompts.assistant.title",
    description: "prompts.assistant.description",
  },
};

/// Titre i18n-aware pour un prompt. Predefined -> traduction. Custom -> tel quel.
export function promptTitle(t: TFunction, p: CustomPrompt): string {
  const keys = ID_TO_KEY[p.id];
  if (keys) return t(keys.title);
  return p.title;
}

/// Description i18n-aware. Retourne `null` si le prompt n'a pas de description.
/// Predefined -> traduction. Custom -> description user telle quelle.
export function promptDescription(
  t: TFunction,
  p: CustomPrompt,
): string | null {
  const keys = ID_TO_KEY[p.id];
  if (keys) return t(keys.description);
  return p.description ?? null;
}
