import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import {
  Check,
  Cpu,
  Download,
  Loader2,
  Trash2,
  Upload,
  Zap,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { cn } from "@/lib/utils";
import { localModelText, formatModelBytes } from "@/lib/modelLabels";
import {
  api,
  type GgufModelState,
  type GpuInfo,
  type LlamaCppSettings,
} from "@/lib/tauri";

type DownloadProgress = { id: string; downloaded: number; total: number };

export function LlmLocalPanel() {
  const { t, i18n } = useTranslation();
  const formatBytes = (bytes: number) =>
    formatModelBytes(bytes, i18n.resolvedLanguage ?? "en");
  const [models, setModels] = useState<GgufModelState[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [cudaEnabled, setCudaEnabled] = useState(false);
  const [gpu, setGpu] = useState<GpuInfo | null>(null);
  const [settings, setSettings] = useState<LlamaCppSettings>({
    n_gpu_layers: 0,
    context_size: 4096,
    max_tokens: 1024,
  });
  const [progress, setProgress] = useState<Record<string, DownloadProgress>>({});
  const [status, setStatus] = useState<Record<string, string>>({});

  useEffect(() => {
    refresh();
    const unProg = listen<DownloadProgress>(
      "llm_model:download:progress",
      (e) =>
        setProgress((prev) => ({ ...prev, [e.payload.id]: e.payload })),
    );
    const unDone = listen<{ id: string; path: string }>(
      "llm_model:download:complete",
      (e) => {
        setProgress((prev) => {
          const { [e.payload.id]: _, ...rest } = prev;
          return rest;
        });
        refresh();
      },
    );
    const unErr = listen<{ id: string; message: string }>(
      "llm_model:download:error",
      (e) => {
        setProgress((prev) => {
          const { [e.payload.id]: _, ...rest } = prev;
          return rest;
        });
        setStatus((s) => ({
          ...s,
          [e.payload.id]: e.payload.message.includes("annule")
            ? t("llmLocal.cancelled")
            : t("llmLocal.errorPrefix", { message: e.payload.message }),
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
      const [list, sel, cuda, g, st] = await Promise.all([
        api.listGgufModels(),
        api.getSelectedGguf(),
        api.llamacppCudaEnabled(),
        api.getGpuInfo(),
        api.getLlamacppSettings(),
      ]);
      setModels(list);
      setSelectedId(sel);
      setCudaEnabled(cuda);
      setGpu(g);
      setSettings(st);
    } catch (e) {
      console.error(e);
    }
  }

  async function download(id: string) {
    if (progress[id]) return;
    // Optimistic progress entry so the button flips to "Annuler" before
    // the first real progress event arrives. Protects against
    // double-click.
    setProgress((p) => ({ ...p, [id]: { id, downloaded: 0, total: 0 } }));
    setStatus((s) => ({ ...s, [id]: t("llmLocal.downloading") }));
    try {
      await api.downloadGgufModel(id);
      setStatus((s) => ({ ...s, [id]: "" }));
    } catch (e) {
      setProgress((p) => {
        const { [id]: _, ...rest } = p;
        return rest;
      });
      setStatus((s) => ({ ...s, [id]: t("llmLocal.errorPrefix", { message: String(e) }) }));
    }
  }

  async function cancelDownload(id: string) {
    await api.cancelDownloadGgufModel(id);
    // Backend emits llm_model:download:error which triggers UI cleanup.
  }

  async function remove(id: string) {
    if (!confirm(t("llmLocal.confirmDelete", { id }))) return;
    try {
      await api.deleteGgufModel(id);
      await refresh();
    } catch (e) {
      setStatus((s) => ({ ...s, [id]: t("llmLocal.errorPrefix", { message: String(e) }) }));
    }
  }

  async function importModel() {
    try {
      await api.importGgufModel();
      await refresh();
    } catch (e) {
      setStatus((s) => ({ ...s, import: t("llmLocal.errorPrefix", { message: String(e) }) }));
    }
  }

  async function select(id: string | null) {
    try {
      await api.setSelectedGguf(id);
      setSelectedId(id);
    } catch (e) {
      console.error(e);
    }
  }

  async function updateSetting(patch: Partial<LlamaCppSettings>) {
    const next = { ...settings, ...patch };
    setSettings(next);
    try {
      await api.setLlamacppSettings(next);
    } catch (e) {
      console.error(e);
    }
  }

  return (
    <Card>
      <CardHeader>
        <div className="flex items-center gap-2">
          <Cpu className="h-4 w-4 text-muted-foreground" />
          <CardTitle className="text-base">{t("llmLocal.title")}</CardTitle>
        </div>
        <CardDescription>{t("llmLocal.description")}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="flex flex-wrap items-center gap-2 rounded-md border p-3 text-xs">
          {cudaEnabled ? (
            <span className="flex items-center gap-1 font-medium text-green-600 dark:text-green-400">
              <Zap className="h-3.5 w-3.5" /> {t("llmLocal.cudaBuild")}
            </span>
          ) : (
            <span className="text-muted-foreground">
              {t("llmLocal.cpuBuildPrefix")}<code>cuda-llama</code>{t("llmLocal.cpuBuildSuffix")}
            </span>
          )}
          {gpu?.has_nvidia && (
            <span className="text-muted-foreground">
              {t("llmLocal.gpuDetected", {
                device: gpu.device_name ?? "",
                cuda: gpu.cuda_version ?? "?",
              })}
            </span>
          )}
        </div>

        <div className="grid gap-2 rounded-md border p-3">
          <p className="text-sm font-medium">{t("llmLocal.inferenceParams")}</p>
          <div className="grid grid-cols-3 gap-3 text-xs">
            <label className="grid gap-1">
              <span className="font-medium">{t("llmLocal.nGpuLayers")}</span>
              <input
                type="number"
                min={0}
                max={200}
                value={settings.n_gpu_layers}
                onChange={(e) =>
                  updateSetting({
                    n_gpu_layers: Number(e.target.value) || 0,
                  })
                }
                className="h-8 rounded-md border border-input bg-background px-2"
              />
              <span className="text-muted-foreground">
                {t("llmLocal.nGpuLayersHelp")}
              </span>
            </label>
            <label className="grid gap-1">
              <span className="font-medium">{t("llmLocal.contextSize")}</span>
              <input
                type="number"
                min={512}
                max={131072}
                step={512}
                value={settings.context_size}
                onChange={(e) =>
                  updateSetting({
                    context_size: Number(e.target.value) || 4096,
                  })
                }
                className="h-8 rounded-md border border-input bg-background px-2"
              />
            </label>
            <label className="grid gap-1">
              <span className="font-medium">{t("llmLocal.maxTokens")}</span>
              <input
                type="number"
                min={32}
                max={8192}
                step={32}
                value={settings.max_tokens}
                onChange={(e) =>
                  updateSetting({
                    max_tokens: Number(e.target.value) || 1024,
                  })
                }
                className="h-8 rounded-md border border-input bg-background px-2"
              />
            </label>
          </div>
        </div>

        <div className="flex items-center justify-between">
          <p className="text-sm font-medium">{t("llmLocal.ggufModels")}</p>
          <Button size="sm" variant="outline" onClick={importModel}>
            <Upload className="h-3.5 w-3.5" />
            {t("llmLocal.importGguf")}
          </Button>
        </div>

        <ul className="grid gap-2">
          {models.map((m) => {
            const text = localModelText(t, "llm", m);
            const prog = progress[m.id];
            const st = status[m.id];
            const isSelected = m.id === selectedId;
            const pct = prog && prog.total > 0
              ? Math.round((prog.downloaded / prog.total) * 100)
              : null;
            return (
              <li
                key={m.id}
                className={cn(
                  "rounded-md border p-3",
                  isSelected && "border-primary/60 bg-primary/5",
                  m.downloaded && !isSelected && "border-green-500/30",
                )}
              >
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <p className="truncate text-sm font-medium">
                      {text.name}
                      {m.imported && (
                        <span className="ml-2 text-[10px] text-muted-foreground">
                          {t("llmLocal.imported")}
                        </span>
                      )}
                      {isSelected && (
                        <span className="ml-2 text-[10px] text-primary">
                          {t("llmLocal.active")}
                        </span>
                      )}
                    </p>
                    <p className="truncate text-xs text-muted-foreground">
                      {text.notes}
                    </p>
                    <p className="text-[11px] text-muted-foreground">
                      {formatBytes(m.size_bytes)}
                      {m.context_length > 0 &&
                        ` - ${t("llmLocal.ctx", {
                          count: m.context_length.toLocaleString(i18n.resolvedLanguage),
                        })}`}
                    </p>
                  </div>
                  <div className="flex shrink-0 gap-1">
                    {m.downloaded && !isSelected && (
                      <Button size="sm" onClick={() => select(m.id)}>
                        <Check className="h-3.5 w-3.5" />
                        {t("llmLocal.choose")}
                      </Button>
                    )}
                    {m.downloaded && isSelected && (
                      <Button
                        size="sm"
                        variant="outline"
                        onClick={() => select(null)}
                      >
                        {t("llmLocal.deactivate")}
                      </Button>
                    )}
                    {!m.downloaded && !prog && (
                      <Button size="sm" onClick={() => download(m.id)}>
                        <Download className="h-3.5 w-3.5" />
                        {t("llmLocal.download")}
                      </Button>
                    )}
                    {prog && (
                      <Button
                        size="sm"
                        variant="ghost"
                        onClick={() => cancelDownload(m.id)}
                      >
                        <Loader2 className="h-3.5 w-3.5 animate-spin" />
                        {t("llmLocal.cancel")}
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
                    </p>
                  </div>
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
        {status.import && (
          <p className="text-xs text-destructive">{status.import}</p>
        )}
      </CardContent>
    </Card>
  );
}
