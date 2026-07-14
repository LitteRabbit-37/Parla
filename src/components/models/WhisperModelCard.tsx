// Card modele Whisper local : badges (EN, importe, installe), tailles,
// download/progress/cancel/delete, et le bouton d'action a 3 etats
// VoiceInk : Download -> Set as Default -> Default Model.

import { useTranslation } from "react-i18next";
import { Check, Download, Loader2, Trash2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { RatingDots } from "@/components/RatingDots";
import { cn } from "@/lib/utils";
import type { DownloadProgress, WhisperModelState } from "@/lib/tauri";

function formatBytes(bytes: number | null | undefined): string {
  if (bytes == null) return "-";
  if (bytes >= 1_000_000_000) return `${(bytes / 1_000_000_000).toFixed(1)} Go`;
  if (bytes >= 1_000_000) return `${(bytes / 1_000_000).toFixed(0)} Mo`;
  return `${Math.round(bytes / 1024)} Ko`;
}

type Props = {
  model: WhisperModelState;
  isCurrent: boolean;
  progress: DownloadProgress | null;
  error: string | null;
  onDownload: () => void;
  onCancelDownload: () => void;
  onDelete: () => void;
  onSetDefault: () => void;
};

export function WhisperModelCard({
  model: m,
  isCurrent,
  progress: p,
  error,
  onDownload,
  onCancelDownload,
  onDelete,
  onSetDefault,
}: Props) {
  const { t } = useTranslation();
  const pct = p ? Math.round((p.downloaded / Math.max(1, p.total)) * 100) : null;
  return (
    <div
      className={cn(
        "rounded-lg border p-4 transition-colors",
        isCurrent && "border-primary bg-primary/5",
      )}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="flex-1">
          <div className="flex items-center gap-2">
            <p className="font-medium">{m.display_name}</p>
            {!m.multilingual && !m.imported && (
              <span className="rounded bg-muted px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground">
                {t("whisperModels.englishOnly")}
              </span>
            )}
            {m.imported && (
              <span className="rounded bg-primary/15 px-1.5 py-0.5 text-[10px] font-medium text-primary">
                {t("whisperModels.importedBadge")}
              </span>
            )}
            {m.downloaded && (
              <span className="flex items-center gap-1 text-[11px] text-green-600 dark:text-green-400">
                <Check className="h-3 w-3" /> {t("whisperModels.installedBadge")}
              </span>
            )}
          </div>
          <p className="mt-0.5 text-xs text-muted-foreground">{m.notes}</p>
          <p className="mt-1 text-xs text-muted-foreground">
            {t("whisperModels.approxSize", { size: formatBytes(m.size_bytes) })}
            {m.downloaded &&
              m.on_disk_bytes != null &&
              ` - ${t("whisperModels.onDisk", { size: formatBytes(m.on_disk_bytes) })}`}
          </p>
          {m.speed > 0 && (
            <div className="mt-1 flex flex-wrap items-center gap-3 text-[11px] text-muted-foreground">
              <span className="inline-flex items-center gap-1">
                {t("common.speed")}
                <RatingDots value={m.speed} />
              </span>
              <span className="inline-flex items-center gap-1">
                {t("common.accuracy")}
                <RatingDots value={m.accuracy} />
              </span>
            </div>
          )}
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
                {t("whisperModels.delete")}
              </Button>
            </>
          ) : p ? (
            <>
              <div className="flex items-center gap-1.5 text-xs">
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                {pct}%
              </div>
              <Button size="sm" variant="outline" onClick={onCancelDownload}>
                <X className="h-3.5 w-3.5" />
                {t("whisperModels.cancel")}
              </Button>
            </>
          ) : (
            <Button size="sm" onClick={onDownload}>
              <Download className="h-3.5 w-3.5" />
              {t("whisperModels.download")}
            </Button>
          )}
        </div>
      </div>
      {p && (
        <div className="mt-2 h-1.5 w-full overflow-hidden rounded-full bg-muted">
          <div
            className="h-full bg-primary transition-all"
            style={{ width: `${pct ?? 0}%` }}
          />
        </div>
      )}
      {error && (
        <p className="mt-2 text-xs text-destructive">
          {t("whisperModels.errorPrefix", { message: error })}
        </p>
      )}
    </div>
  );
}
