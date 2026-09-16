// Delai maximum d'une transcription cloud (batch).
//
// Reference VoiceInk Features/ModelLibrary/Views/ModelSettingsPanel.swift +
// AppDefaults `CloudTranscriptionSettings` : defaut 30 s, plage 10 s a
// 30 min (commit 1bab779 "Add configurable cloud transcription timeout").

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Timer } from "lucide-react";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { api } from "@/lib/tauri";

const MIN_SECS = 10;
const MAX_SECS = 30 * 60;

export function CloudTimeoutPanel() {
  const { t } = useTranslation();
  const [secs, setSecs] = useState<number>(30);

  useEffect(() => {
    api.getCloudTranscriptionTimeout().then(setSecs).catch(console.error);
  }, []);

  async function save(next: number) {
    const clamped = Math.round(
      Math.max(MIN_SECS, Math.min(MAX_SECS, Number.isFinite(next) ? next : 30)),
    );
    setSecs(clamped);
    try {
      await api.setCloudTranscriptionTimeout(clamped);
    } catch (e) {
      console.error(e);
    }
  }

  return (
    <Card>
      <CardHeader>
        <div className="flex items-center gap-2">
          <Timer className="h-4 w-4 text-muted-foreground" />
          <CardTitle className="text-base">{t("cloudTimeout.title")}</CardTitle>
        </div>
        <CardDescription>{t("cloudTimeout.description")}</CardDescription>
      </CardHeader>
      <CardContent>
        <div className="flex items-center gap-2">
          <input
            type="number"
            min={MIN_SECS}
            max={MAX_SECS}
            step={5}
            value={secs}
            onChange={(e) =>
              setSecs(
                Number.isFinite(e.target.valueAsNumber)
                  ? e.target.valueAsNumber
                  : MIN_SECS,
              )
            }
            onBlur={(e) => save(e.target.valueAsNumber)}
            className="h-9 w-28 rounded-md border border-input bg-background px-3 text-sm"
          />
          <span className="text-xs text-muted-foreground">
            {t("common.seconds")} ({MIN_SECS} - {MAX_SECS})
          </span>
        </div>
      </CardContent>
    </Card>
  );
}
