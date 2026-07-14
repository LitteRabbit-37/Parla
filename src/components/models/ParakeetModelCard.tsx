// Card modele Parakeet : meta int8/F16, multilingue, missing files,
// download avec fichier courant, et le bouton d'action a 3 etats
// VoiceInk : Download -> Set as Default -> Default Model.

import { useTranslation } from "react-i18next";
import { Cpu, Download, Languages, Loader2, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { RatingDots } from "@/components/RatingDots";
import { cn } from "@/lib/utils";
import type { ParakeetModelState } from "@/lib/tauri";
import type { ParakeetDownloadProgress } from "./types";

function formatBytes(b: number): string {
  if (b < 1024) return `${b} B`;
  if (b < 1024 * 1024) return `${(b / 1024).toFixed(1)} KB`;
  if (b < 1024 * 1024 * 1024) return `${(b / 1024 / 1024).toFixed(1)} MB`;
  return `${(b / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

type Props = {
  model: ParakeetModelState;
  isCurrent: boolean;
  progress: ParakeetDownloadProgress | null;
  status: string | null;
  onDownload: () => void;
  onCancelDownload: () => void;
  onDelete: () => void;
  onSetDefault: () => void;
};

export function ParakeetModelCard({
  model: m,
  isCurrent,
  progress: prog,
  status: st,
  onDownload,
  onCancelDownload,
  onDelete,
  onSetDefault,
}: Props) {
  const { t } = useTranslation();
  const pct =
    prog && prog.total > 0
      ? Math.round((prog.downloaded / prog.total) * 100)
      : null;
  return (
    <div
      className={cn(
        "rounded-lg border p-4 transition-colors",
        isCurrent && "border-primary bg-primary/5",
      )}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
          <p className="truncate font-medium">{m.display_name}</p>
          <p className="mt-0.5 text-xs text-muted-foreground">{m.notes}</p>
          <div className="mt-1 flex flex-wrap items-center gap-3 text-[11px] text-muted-foreground">
            <span className="inline-flex items-center gap-1">
              <Cpu className="h-3 w-3" />
              {m.is_quantized ? "int8" : "F16"}
            </span>
            <span className="inline-flex items-center gap-1">
              <Languages className="h-3 w-3" />
              {m.multilingual
                ? t("parakeet.multilingual")
                : t("parakeet.englishOnly")}
            </span>
            <span>{formatBytes(m.size_bytes)}</span>
            <span className="inline-flex items-center gap-1">
              {t("common.speed")}
              <RatingDots value={m.speed} />
            </span>
            <span className="inline-flex items-center gap-1">
              {t("common.accuracy")}
              <RatingDots value={m.accuracy} />
            </span>
          </div>
        </div>
        <div className="flex shrink-0 flex-col items-end gap-1.5">
          {m.downloaded ? (
            <>
              {isCurrent ? (
                <span className="px-3 py-1.5 text-xs font-medium text-muted-foreground">
                  {t("aiModels.defaultModel")}
                </span>
              ) : (
                <Button size="sm" variant="outline" onClick={onSetDefault}>
                  {t("aiModels.setAsDefault")}
                </Button>
              )}
              <Button size="sm" variant="ghost" onClick={onDelete}>
                <Trash2 className="h-3.5 w-3.5" />
                {t("common.delete")}
              </Button>
            </>
          ) : prog ? (
            <Button size="sm" variant="ghost" onClick={onCancelDownload}>
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
              {t("parakeet.cancel")}
            </Button>
          ) : (
            <Button size="sm" onClick={onDownload}>
              <Download className="h-3.5 w-3.5" />
              {t("parakeet.download")}
            </Button>
          )}
        </div>
      </div>
      {prog && pct !== null && (
        <div className="mt-2">
          <div className="h-1.5 w-full overflow-hidden rounded-full bg-muted">
            <div
              className="h-full bg-primary transition-[width]"
              style={{ width: `${pct}%` }}
            />
          </div>
          <p className="mt-1 text-[11px] text-muted-foreground">
            {formatBytes(prog.downloaded)} / {formatBytes(prog.total)} ({pct}%)
            {" - "}
            {prog.current_file}
          </p>
        </div>
      )}
      {!m.downloaded && m.missing_files.length > 0 && !prog && (
        <p className="mt-1 text-[11px] text-muted-foreground">
          {t("parakeet.missingFiles", { files: m.missing_files.join(", ") })}
        </p>
      )}
      {st && (
        <p
          className={cn(
            "mt-2 text-xs",
            st.startsWith(t("common.error"))
              ? "text-destructive"
              : "text-muted-foreground",
          )}
        >
          {st}
        </p>
      )}
    </div>
  );
}
