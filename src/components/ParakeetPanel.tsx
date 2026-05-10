import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import {
  Check,
  Cpu,
  Download,
  Languages,
  Loader2,
  Mic,
  Trash2,
} from "lucide-react";
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
import { api, type ParakeetModelState } from "@/lib/tauri";

type DownloadProgress = {
  id: string;
  downloaded: number;
  total: number;
  current_file: string;
};

function formatBytes(b: number): string {
  if (b < 1024) return `${b} B`;
  if (b < 1024 * 1024) return `${(b / 1024).toFixed(1)} KB`;
  if (b < 1024 * 1024 * 1024) return `${(b / 1024 / 1024).toFixed(1)} MB`;
  return `${(b / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

export function ParakeetPanel() {
  const { t } = useTranslation();
  const [models, setModels] = useState<ParakeetModelState[]>([]);
  const [ep, setEp] = useState<string>("cpu");
  const [activeId, setActiveId] = useState<string | null>(null);
  const [progress, setProgress] = useState<Record<string, DownloadProgress>>(
    {},
  );
  const [status, setStatus] = useState<Record<string, string>>({});

  useEffect(() => {
    refresh();
    const unProg = listen<DownloadProgress>(
      "parakeet_model:download:progress",
      (e) => setProgress((p) => ({ ...p, [e.payload.id]: e.payload })),
    );
    const unDone = listen<{ id: string; path: string }>(
      "parakeet_model:download:complete",
      (e) => {
        setProgress((p) => {
          const { [e.payload.id]: _, ...rest } = p;
          return rest;
        });
        refresh();
      },
    );
    // Backend emits this on cancellation, HTTP failure, or any
    // download_impl error. We wipe the progress bar + show the message so
    // the user can click Telecharger again without refreshing the view.
    const unErr = listen<{ id: string; message: string }>(
      "parakeet_model:download:error",
      (e) => {
        setProgress((p) => {
          const { [e.payload.id]: _, ...rest } = p;
          return rest;
        });
        setStatus((s) => ({
          ...s,
          [e.payload.id]: e.payload.message.includes("annule")
            ? t("parakeet.cancelled")
            : t("parakeet.errorPrefix", { message: e.payload.message }),
        }));
      },
    );
    return () => {
      unProg.then((fn) => fn());
      unDone.then((fn) => fn());
      unErr.then((fn) => fn());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function refresh() {
    try {
      const [list, e, src] = await Promise.all([
        api.listParakeetModels(),
        api.parakeetExecutionProvider(),
        api.getTranscriptionSource(),
      ]);
      setModels(list);
      setEp(e);
      setActiveId(src.kind === "parakeet" ? src.parakeet_model_id : null);
    } catch (e) {
      console.error(e);
    }
  }

  async function download(id: string) {
    // Optimistic progress entry prevents double-click : as soon as the
    // user clicks, `progress[id]` is set, the button flips to "Annuler"
    // on next render, and the backend's own reentrancy guard rejects any
    // in-flight duplicate anyway.
    setProgress((p) => ({
      ...p,
      [id]: { id, downloaded: 0, total: 0, current_file: "" },
    }));
    setStatus((s) => ({ ...s, [id]: t("parakeet.downloading") }));
    try {
      await api.downloadParakeetModel(id);
      setStatus((s) => ({ ...s, [id]: "" }));
    } catch (e) {
      // The error event handler already wipes `progress` and sets a
      // status message, so this catch is mostly a safety net for
      // unexpected rejections (never reached in normal flow).
      setProgress((p) => {
        const { [id]: _, ...rest } = p;
        return rest;
      });
      setStatus((s) => ({ ...s, [id]: t("parakeet.errorPrefix", { message: String(e) }) }));
    }
  }

  async function cancelDl(id: string) {
    await api.cancelDownloadParakeetModel(id);
    // The backend emits parakeet_model:download:error after wrapping up
    // the cancel, which triggers the UI cleanup in the listen handler.
  }

  async function remove(id: string) {
    if (!confirm(t("parakeet.confirmDelete", { id }))) return;
    try {
      await api.deleteParakeetModel(id);
      await refresh();
    } catch (e) {
      setStatus((s) => ({ ...s, [id]: t("parakeet.errorPrefix", { message: String(e) }) }));
    }
  }

  async function activate(id: string) {
    await api.setTranscriptionSource({
      kind: "parakeet",
      parakeet_model_id: id,
    });
    setActiveId(id);
  }

  return (
    <Card>
      <CardHeader>
        <div className="flex items-center gap-2">
          <Mic className="h-4 w-4 text-muted-foreground" />
          <CardTitle className="text-base">{t("parakeet.title")}</CardTitle>
        </div>
        <CardDescription>{t("parakeet.description")}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="flex flex-wrap items-center gap-2 rounded-md border p-3 text-xs">
          <span className="font-medium">{t("parakeet.executionProvider")}</span>
          <code>{ep}</code>
          {ep === "cpu" && (
            <span className="text-muted-foreground">
              {t("parakeet.cpuHint")}
            </span>
          )}
        </div>

        <ul className="grid gap-2">
          {models.map((m) => {
            const prog = progress[m.id];
            const st = status[m.id];
            const isActive = m.id === activeId;
            const pct = prog && prog.total > 0
              ? Math.round((prog.downloaded / prog.total) * 100)
              : null;
            return (
              <li
                key={m.id}
                className={cn(
                  "rounded-md border p-3",
                  isActive && "border-primary/60 bg-primary/5",
                  m.downloaded && !isActive && "border-green-500/30",
                )}
              >
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <p className="truncate text-sm font-medium">
                      {m.display_name}
                      {isActive && (
                        <span className="ml-2 text-[10px] text-primary">
                          {t("parakeet.active")}
                        </span>
                      )}
                    </p>
                    <p className="text-[11px] text-muted-foreground">
                      {m.notes}
                    </p>
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
                  <div className="flex shrink-0 gap-1">
                    {m.downloaded && !isActive && (
                      <Button size="sm" onClick={() => activate(m.id)}>
                        <Check className="h-3.5 w-3.5" />
                        {t("parakeet.activate")}
                      </Button>
                    )}
                    {!m.downloaded && !prog && (
                      <Button size="sm" onClick={() => download(m.id)}>
                        <Download className="h-3.5 w-3.5" />
                        {t("parakeet.download")}
                      </Button>
                    )}
                    {prog && (
                      <Button
                        size="sm"
                        variant="ghost"
                        onClick={() => cancelDl(m.id)}
                      >
                        <Loader2 className="h-3.5 w-3.5 animate-spin" />
                        {t("parakeet.cancel")}
                      </Button>
                    )}
                    {m.downloaded && (
                      <Button
                        size="sm"
                        variant="ghost"
                        onClick={() => remove(m.id)}
                      >
                        <Trash2 className="h-3.5 w-3.5" />
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
                      {formatBytes(prog.downloaded)} /
                      {" "}{formatBytes(prog.total)} ({pct}%)
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
              </li>
            );
          })}
        </ul>
      </CardContent>
    </Card>
  );
}
