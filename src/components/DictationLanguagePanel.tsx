// Selecteur de langue de dictee, filtre par les langues supportees par
// la source de transcription active (whisper / parakeet / cloud).
//
// Reference VoiceInk : Views/AI Models/LanguageSelectionView.swift et
// LanguageDictionary.swift. La cle store cote Parla est `whisper_language`
// (legacy : la cle existait avant les sources cloud / parakeet).

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { Globe } from "lucide-react";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { languageLabel } from "@/lib/languages";
import { api, type TranscriptionSource } from "@/lib/tauri";

export function DictationLanguagePanel() {
  const { t } = useTranslation();
  const [supportedCodes, setSupportedCodes] = useState<string[]>([]);
  const [selected, setSelected] = useState<string>("auto");
  const [activeLabel, setActiveLabel] = useState<string>("");

  useEffect(() => {
    refresh();
    const un = listen<TranscriptionSource>("source:changed", () => refresh());
    return () => {
      un.then((fn) => fn());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function refresh() {
    try {
      const [src, currentLang] = await Promise.all([
        api.getTranscriptionSource(),
        api.getDictationLanguage(),
      ]);
      const codes = await resolveSupportedCodes(src);
      setSupportedCodes(codes);
      setActiveLabel(formatActiveModel(src));
      // Si la langue actuelle n'est pas supportee par le modele actif, on
      // retombe sur "auto" si dispo, sinon sur le premier code de la liste
      // (typiquement "en"). Cf VoiceInk validLanguageOrFallback.
      if (codes.length > 0 && !codes.includes(currentLang)) {
        const fallback = codes.includes("auto") ? "auto" : codes[0];
        setSelected(fallback);
        await api.setDictationLanguage(fallback);
      } else {
        setSelected(currentLang);
      }
    } catch (e) {
      console.error(e);
    }
  }

  async function change(code: string) {
    setSelected(code);
    try {
      await api.setDictationLanguage(code);
    } catch (e) {
      console.error(e);
    }
  }

  // Pas d'affichage tant qu'on n'a pas de codes (source non configuree).
  if (supportedCodes.length === 0) return null;

  // Tri : "auto" toujours en premier, puis les autres alphabetiquement
  // par libelle affichable. La liste source est deja ISO-canonique mais
  // les noms varient.
  const sorted = [...supportedCodes].sort((a, b) => {
    if (a === "auto") return -1;
    if (b === "auto") return 1;
    return languageLabel(a).localeCompare(languageLabel(b));
  });

  return (
    <Card>
      <CardHeader>
        <div className="flex items-center gap-2">
          <Globe className="h-4 w-4 text-muted-foreground" />
          <CardTitle className="text-base">
            {t("dictationLanguage.title")}
          </CardTitle>
        </div>
        <CardDescription>
          {t("dictationLanguage.description", { model: activeLabel })}
        </CardDescription>
      </CardHeader>
      <CardContent>
        <select
          value={selected}
          onChange={(e) => change(e.target.value)}
          className="flex h-9 w-full max-w-md rounded-md border border-input bg-background px-3 py-1 text-sm shadow-sm"
        >
          {sorted.map((code) => (
            <option key={code} value={code}>
              {code === "auto"
                ? t("dictationLanguage.auto")
                : languageLabel(code)}
              {" "}
              ({code})
            </option>
          ))}
        </select>
      </CardContent>
    </Card>
  );
}

async function resolveSupportedCodes(
  src: TranscriptionSource,
): Promise<string[]> {
  if (src.kind === "local" && src.whisper_model_id) {
    const list = await api.listWhisperModels();
    return list.find((m) => m.id === src.whisper_model_id)?.language_codes ?? [];
  }
  if (src.kind === "parakeet" && src.parakeet_model_id) {
    const list = await api.listParakeetModels();
    return (
      list.find((m) => m.id === src.parakeet_model_id)?.language_codes ?? []
    );
  }
  if (src.kind === "cloud" && src.cloud_provider && src.cloud_model) {
    const list = await api.listCloudModels();
    return (
      list.find(
        (m) =>
          m.provider_id === src.cloud_provider && m.model_id === src.cloud_model,
      )?.language_codes ?? []
    );
  }
  return [];
}

function formatActiveModel(src: TranscriptionSource): string {
  if (src.kind === "local") return src.whisper_model_id ?? "?";
  if (src.kind === "parakeet") return src.parakeet_model_id ?? "?";
  if (src.kind === "cloud") {
    return `${src.cloud_provider ?? "?"} / ${src.cloud_model ?? "?"}`;
  }
  return "?";
}
