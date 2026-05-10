// Cloud providers : configuration des cles API ET selection des modeles.
//
// Ordre clarifie suite a feedback : 1) cles API en haut, 2) liste de modeles
// selectables en bas (active uniquement si cle API configuree pour le provider).

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  Check,
  ExternalLink,
  Key,
  ListChecks,
  Loader2,
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
import { api, type TranscriptionSource as Source } from "@/lib/tauri";

type Provider = {
  id: string;
  display_name: string;
  requires_api_key: boolean;
  api_key_url: string;
  has_api_key: boolean;
};

type CloudModel = {
  provider_id: string;
  model_id: string;
  display_name: string;
  supports_batch: boolean;
  supports_streaming: boolean;
  multilingual: boolean;
  notes: string;
  speed: number;
  accuracy: number;
};

export function CloudProvidersPanel() {
  const { t } = useTranslation();
  const [providers, setProviders] = useState<Provider[]>([]);
  const [models, setModels] = useState<CloudModel[]>([]);
  const [source, setSource] = useState<Source | null>(null);
  const [inputs, setInputs] = useState<Record<string, string>>({});
  const [verifying, setVerifying] = useState<Record<string, boolean>>({});
  const [status, setStatus] = useState<Record<string, string>>({});

  useEffect(() => {
    refresh();
  }, []);

  async function refresh() {
    try {
      const [list, mods, src] = await Promise.all([
        api.listCloudProviders(),
        api.listCloudModels(),
        api.getTranscriptionSource(),
      ]);
      setProviders(list);
      setModels(mods);
      setSource(src);
    } catch (e) {
      console.error(e);
    }
  }

  async function save(p: Provider) {
    const key = (inputs[p.id] ?? "").trim();
    if (!key) return;
    setVerifying((v) => ({ ...v, [p.id]: true }));
    setStatus((s) => ({ ...s, [p.id]: t("cloud.verifying") }));
    try {
      await api.verifyApiKey(p.id, key);
      await api.setApiKey(p.id, key);
      setStatus((s) => ({ ...s, [p.id]: t("cloud.keySaved") }));
      setInputs((i) => ({ ...i, [p.id]: "" }));
      await refresh();
    } catch (e) {
      setStatus((s) => ({ ...s, [p.id]: t("cloud.errorPrefix", { message: String(e) }) }));
    } finally {
      setVerifying((v) => ({ ...v, [p.id]: false }));
    }
  }

  async function remove(p: Provider) {
    try {
      await api.deleteApiKey(p.id);
      setStatus((s) => ({ ...s, [p.id]: t("cloud.keyDeleted") }));
      await refresh();
    } catch (e) {
      setStatus((s) => ({ ...s, [p.id]: t("cloud.errorPrefix", { message: String(e) }) }));
    }
  }

  async function selectCloudModel(m: CloudModel) {
    if (!source) return;
    const next: Source = {
      kind: "cloud",
      whisper_model_id: source.whisper_model_id,
      cloud_provider: m.provider_id,
      cloud_model: m.model_id,
      parakeet_model_id: source.parakeet_model_id,
    };
    setSource(next);
    try {
      await api.setTranscriptionSource(next);
    } catch (e) {
      console.error(e);
    }
  }

  const providersWithKeys = new Set(
    providers.filter((p) => p.has_api_key).map((p) => p.id),
  );
  const batchOnly = models.filter((m) => m.supports_batch);

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <div className="flex items-center gap-2">
            <Key className="h-4 w-4 text-muted-foreground" />
            <CardTitle className="text-base">{t("cloud.keysTitle")}</CardTitle>
          </div>
          <CardDescription>{t("cloud.keysDescription")}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          {providers.map((p) => {
            const st = status[p.id];
            const busy = verifying[p.id];
            return (
              <div
                key={p.id}
                className={cn(
                  "rounded-lg border p-3",
                  p.has_api_key && "border-green-500/40 bg-green-500/5",
                )}
              >
                <div className="flex items-center justify-between gap-3">
                  <div className="flex items-center gap-2">
                    <p className="font-medium">{p.display_name}</p>
                    {p.has_api_key && (
                      <span className="flex items-center gap-1 text-[11px] text-green-600 dark:text-green-400">
                        <Check className="h-3 w-3" /> {t("cloud.configured")}
                      </span>
                    )}
                  </div>
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => openUrl(p.api_key_url)}
                    title={p.api_key_url}
                  >
                    <ExternalLink className="h-3.5 w-3.5" />
                    {t("cloud.getKey")}
                  </Button>
                </div>
                <div className="mt-3 grid grid-cols-[1fr_auto_auto] gap-2">
                  <input
                    type="password"
                    placeholder={
                      p.has_api_key
                        ? t("cloud.apiKeySavedPlaceholder")
                        : t("cloud.apiKeyPlaceholder")
                    }
                    value={inputs[p.id] ?? ""}
                    onChange={(e) =>
                      setInputs((i) => ({ ...i, [p.id]: e.target.value }))
                    }
                    className="flex h-9 rounded-md border border-input bg-background px-3 text-sm shadow-sm"
                    autoComplete="off"
                  />
                  <Button size="sm" onClick={() => save(p)} disabled={busy}>
                    {busy ? <Loader2 className="animate-spin" /> : null}
                    {t("cloud.verifyAndSave")}
                  </Button>
                  {p.has_api_key && (
                    <Button size="sm" variant="ghost" onClick={() => remove(p)}>
                      <Trash2 className="h-3.5 w-3.5" />
                    </Button>
                  )}
                </div>
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
          })}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <div className="flex items-center gap-2">
            <ListChecks className="h-4 w-4 text-muted-foreground" />
            <CardTitle className="text-base">{t("cloud.modelsTitle")}</CardTitle>
          </div>
          <CardDescription>{t("cloud.modelsDescription")}</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="divide-y rounded-md border">
            {batchOnly.map((m) => {
              const configured = providersWithKeys.has(m.provider_id);
              const selected =
                source?.cloud_provider === m.provider_id &&
                source?.cloud_model === m.model_id;
              return (
                <button
                  key={`${m.provider_id}:${m.model_id}`}
                  onClick={() => selectCloudModel(m)}
                  disabled={!configured}
                  className={cn(
                    "flex w-full items-start gap-3 p-3 text-left transition-colors",
                    selected && "bg-primary/10",
                    !configured && "cursor-not-allowed opacity-60",
                    configured && !selected && "hover:bg-accent/40",
                  )}
                >
                  <div className="flex-1">
                    <div className="flex items-center gap-2">
                      <p className="text-sm font-medium">{m.display_name}</p>
                      {m.supports_streaming && (
                        <span className="rounded bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">
                          {t("cloud.streaming")}
                        </span>
                      )}
                      {!configured && (
                        <span className="text-[11px] text-amber-600 dark:text-amber-400">
                          {t("cloud.apiKeyRequired")}
                        </span>
                      )}
                    </div>
                    <p className="text-xs text-muted-foreground">{m.notes}</p>
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
                  </div>
                  {selected && (
                    <span className="text-xs font-medium text-primary">
                      {t("cloud.selected")}
                    </span>
                  )}
                </button>
              );
            })}
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
