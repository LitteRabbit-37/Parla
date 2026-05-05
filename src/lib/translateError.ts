// Mappe les codes d'erreur stables emis par le backend Rust vers des
// chaines i18n traduites cote frontend.
//
// Convention : le backend prefixe ses erreurs user-facing par `PARLA_ERR:`
// suivi d'un identifiant snake-or-camelCase et d'arguments optionnels
// separes par `:`. Exemple : `PARLA_ERR:apiKey:groq` -> i18n key
// `errors.apiKey` interpole avec `{ provider: "groq" }`.
//
// Toute erreur qui ne match pas un code stable est retournee telle quelle :
// les utilisateurs verront le message technique brut (anglais en general
// puisque les wrappers comme reqwest, anyhow, windows-rs sont anglais).
// C'est moins joli mais ca evite de masquer un probleme inattendu derriere
// un libelle generique.

import type { TFunction } from "i18next";

type ErrorMapping = {
  i18nKey: string;
  /** Builds interpolation args from the captured groups in the regex. */
  args?: (groups: string[]) => Record<string, unknown>;
};

const ERROR_MAP: Array<[RegExp, ErrorMapping]> = [
  [/^PARLA_ERR:noWhisper$/, { i18nKey: "errors.noWhisper" }],
  [/^PARLA_ERR:noParakeet$/, { i18nKey: "errors.noParakeet" }],
  [
    /^PARLA_ERR:modelNotDownloaded:(.+)$/,
    {
      i18nKey: "errors.modelNotDownloaded",
      args: (g) => ({ model: g[1] }),
    },
  ],
  [
    /^PARLA_ERR:parakeetIncomplete:(.+)$/,
    {
      i18nKey: "errors.parakeetIncomplete",
      args: (g) => ({ model: g[1] }),
    },
  ],
  [
    /^PARLA_ERR:apiKey:(.+)$/,
    {
      i18nKey: "errors.apiKeyMissing",
      args: (g) => ({ provider: g[1] }),
    },
  ],
  [/^PARLA_ERR:cloudUnconfigured$/, { i18nKey: "errors.cloudUnconfigured" }],
];

/// Traduit un message d'erreur backend en chaine localisee. Si le code
/// n'est pas reconnu, retourne le message d'origine sans interpretation.
export function translateError(t: TFunction, raw: string | null | undefined): string {
  if (!raw) return "";
  for (const [pattern, mapping] of ERROR_MAP) {
    const match = pattern.exec(raw);
    if (!match) continue;
    const args = mapping.args ? mapping.args(match.slice(1)) : undefined;
    return t(mapping.i18nKey, args);
  }
  return raw;
}
