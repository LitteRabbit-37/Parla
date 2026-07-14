// Card d'import de modele whisper local - replique VoiceInk
// "Import Local Model" (card en fin de liste Local) + InfoTip.
// Dialog fichier .bin -> commande import_whisper_model.

import { useState } from "react";
import { useTranslation } from "react-i18next";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { FolderOpen, Upload } from "lucide-react";
import { Button } from "@/components/ui/button";
import { InfoTip } from "@/components/ui/info-tip";
import { api } from "@/lib/tauri";

export function ImportModelCard({
  onImported,
}: {
  onImported: (id: string) => void;
}) {
  const { t } = useTranslation();
  const [error, setError] = useState<string | null>(null);

  async function importModel() {
    try {
      const selected = await openDialog({
        multiple: false,
        filters: [
          { name: t("whisperModels.dialogFilter"), extensions: ["bin"] },
        ],
        title: t("whisperModels.dialogTitle"),
      });
      if (!selected || typeof selected !== "string") return;
      const newId = await api.importWhisperModel(selected);
      setError(null);
      onImported(newId);
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div className="rounded-lg border border-dashed bg-muted/30 p-4">
      <div className="flex items-center justify-between gap-3">
        <div className="flex items-center gap-2">
          <FolderOpen className="h-4 w-4 text-muted-foreground" />
          <div>
            <div className="flex items-center gap-1.5">
              <p className="text-sm font-medium">{t("aiModels.import.title")}</p>
              <InfoTip learnMoreUrl="https://tryvoiceink.com/docs/custom-local-whisper-models">
                {t("aiModels.import.infoTip")}
              </InfoTip>
            </div>
            <p className="text-xs text-muted-foreground">
              {t("aiModels.import.description")}
            </p>
          </div>
        </div>
        <Button size="sm" variant="outline" onClick={importModel}>
          <Upload className="h-3.5 w-3.5" />
          {t("aiModels.import.browse")}
        </Button>
      </div>
      {error && (
        <p className="mt-2 text-xs text-destructive">
          {t("aiModels.import.error", { message: error })}
        </p>
      )}
    </div>
  );
}
