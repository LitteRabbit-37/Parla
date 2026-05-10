import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { Check, Download, FolderOpen, Loader2, Star, Trash2, Upload, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { RatingDots } from "@/components/RatingDots";
import { cn } from "@/lib/utils";
import {
  api,
  type DownloadComplete,
  type DownloadError,
  type DownloadProgress,
  type WhisperModelState,
} from "@/lib/tauri";

function formatBytes(bytes: number | null | undefined): string {
  if (bytes == null) return "-";
  if (bytes >= 1_000_000_000) return `${(bytes / 1_000_000_000).toFixed(1)} Go`;
  if (bytes >= 1_000_000) return `${(bytes / 1_000_000).toFixed(0)} Mo`;
  return `${Math.round(bytes / 1024)} Ko`;
}

type Props = {
  selectedId: string | null;
  onSelect: (id: string | null) => void;
};

export function ModelsPanel({ selectedId, onSelect }: Props) {
  const { t } = useTranslation();
  const [models, setModels] = useState<WhisperModelState[]>([]);
  const [progress, setProgress] = useState<Record<string, DownloadProgress>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});

  async function refresh() {
    try {
      const list = await api.listWhisperModels();
      setModels(list);
      // Auto-selection : le premier modele telecharge si rien de selectionne.
      if (!selectedId) {
        const first = list.find((m) => m.downloaded);
        if (first) onSelect(first.id);
      }
    } catch (e) {
      console.error(e);
    }
  }

  useEffect(() => {
    refresh();
    const unlisteners = [
      listen<DownloadProgress>("model:download:progress", (e) => {
        setProgress((p) => ({ ...p, [e.payload.id]: e.payload }));
      }),
      listen<DownloadComplete>("model:download:complete", async (e) => {
        setProgress((p) => {
          const next = { ...p };
          delete next[e.payload.id];
          return next;
        });
        setErrors((er) => {
          const next = { ...er };
          delete next[e.payload.id];
          return next;
        });
        await refresh();
        if (!selectedId) onSelect(e.payload.id);
      }),
      listen<DownloadError>("model:download:error", (e) => {
        setProgress((p) => {
          const next = { ...p };
          delete next[e.payload.id];
          return next;
        });
        setErrors((er) => ({ ...er, [e.payload.id]: e.payload.message }));
      }),
    ];
    return () => {
      Promise.all(unlisteners).then((arr) => arr.forEach((fn) => fn()));
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function download(id: string) {
    // Prevent multi-click : optimistic progress entry flips the button to
    // "Annuler" immediately. The backend also has its own reentrancy guard.
    if (progress[id]) return;
    setProgress((p) => ({
      ...p,
      [id]: { id, downloaded: 0, total: 0 },
    }));
    setErrors((er) => {
      const next = { ...er };
      delete next[id];
      return next;
    });
    try {
      await api.downloadWhisperModel(id);
    } catch (e) {
      // Safety net : the model:download:error event normally handles
      // cleanup, but catch stray rejections here too.
      setProgress((p) => {
        const next = { ...p };
        delete next[id];
        return next;
      });
      setErrors((er) => ({ ...er, [id]: String(e) }));
    }
  }

  async function cancelDownload(id: string) {
    await api.cancelDownloadWhisperModel(id);
    // The backend emits model:download:error which triggers UI cleanup.
  }

  async function remove(id: string) {
    try {
      await api.deleteWhisperModel(id);
      if (selectedId === id) onSelect(null);
      await refresh();
    } catch (e) {
      setErrors((er) => ({ ...er, [id]: String(e) }));
    }
  }

  async function importModel() {
    try {
      const selected = await openDialog({
        multiple: false,
        filters: [{ name: t("whisperModels.dialogFilter"), extensions: ["bin"] }],
        title: t("whisperModels.dialogTitle"),
      });
      if (!selected || typeof selected !== "string") return;
      const newId = await api.importWhisperModel(selected);
      await refresh();
      onSelect(newId);
    } catch (e) {
      setErrors((er) => ({ ...er, ["__import__"]: String(e) }));
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">{t("whisperModels.title")}</CardTitle>
        <CardDescription>{t("whisperModels.description")}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="flex items-center justify-between rounded-lg border border-dashed bg-muted/30 p-3">
          <div className="flex items-center gap-2">
            <FolderOpen className="h-4 w-4 text-muted-foreground" />
            <div>
              <p className="text-sm font-medium">{t("whisperModels.importTitle")}</p>
              <p className="text-xs text-muted-foreground">
                {t("whisperModels.importDescription")}
              </p>
            </div>
          </div>
          <Button size="sm" variant="outline" onClick={importModel}>
            <Upload className="h-3.5 w-3.5" />
            {t("whisperModels.browse")}
          </Button>
        </div>
        {errors["__import__"] && (
          <p className="text-xs text-destructive">
            {t("whisperModels.importError", { message: errors["__import__"] })}
          </p>
        )}

        {models.map((m) => {
          const p = progress[m.id];
          const pct = p ? Math.round((p.downloaded / Math.max(1, p.total)) * 100) : null;
          const isSelected = selectedId === m.id;
          const err = errors[m.id];
          return (
            <div
              key={m.id}
              className={cn(
                "rounded-lg border p-3 transition-colors",
                isSelected && "border-primary bg-primary/5",
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
                <div className="flex flex-col items-end gap-1.5">
                  {m.downloaded ? (
                    <>
                      <Button
                        size="sm"
                        variant={isSelected ? "default" : "outline"}
                        onClick={() => onSelect(m.id)}
                      >
                        <Star className="h-3.5 w-3.5" />
                        {isSelected
                          ? t("whisperModels.selected")
                          : t("whisperModels.select")}
                      </Button>
                      <Button size="sm" variant="ghost" onClick={() => remove(m.id)}>
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
                      <Button size="sm" variant="outline" onClick={() => cancelDownload(m.id)}>
                        <X className="h-3.5 w-3.5" />
                        {t("whisperModels.cancel")}
                      </Button>
                    </>
                  ) : (
                    <Button size="sm" onClick={() => download(m.id)}>
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
              {err && (
                <p className="mt-2 text-xs text-destructive">
                  {t("whisperModels.errorPrefix", { message: err })}
                </p>
              )}
            </div>
          );
        })}
      </CardContent>
    </Card>
  );
}

