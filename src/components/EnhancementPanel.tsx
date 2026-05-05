import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, Key, Loader2, Sparkles, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { EnhancementScreenContext } from "@/components/EnhancementScreenContext";
import { PromptEditor } from "@/components/PromptEditor";
import { cn } from "@/lib/utils";
import {
  api,
  type CustomPrompt,
  type LLMProviderInfo,
  type LLMSelection,
} from "@/lib/tauri";
import { promptDescription, promptTitle } from "@/lib/promptLabels";

export function EnhancementPanel() {
  const { t } = useTranslation();
  const [enabled, setEnabled] = useState(false);
  const [providers, setProviders] = useState<LLMProviderInfo[]>([]);
  const [selection, setSelection] = useState<LLMSelection | null>(null);
  const [prompts, setPrompts] = useState<CustomPrompt[]>([]);
  const [activePromptId, setActivePromptId] = useState<string | null>(null);
  const [apiKeyInputs, setApiKeyInputs] = useState<Record<string, string>>({});
  const [verifying, setVerifying] = useState<Record<string, boolean>>({});
  const [status, setStatus] = useState<Record<string, string>>({});
  const [ollamaBaseUrl, setOllamaBaseUrl] = useState("");
  const [ollamaModels, setOllamaModels] = useState<string[]>([]);
  const [ollamaStatus, setOllamaStatus] = useState("");
  const [customBaseUrl, setCustomBaseUrl] = useState("");
  const [customModel, setCustomModel] = useState("");
  const [localcliCustomCmd, setLocalcliCustomCmd] = useState("");
  const [localcliTimeout, setLocalcliTimeout] = useState(45);
  const [localcliStatus, setLocalcliStatus] = useState("");

  useEffect(() => {
    refresh();
  }, []);

  async function refresh() {
    try {
      const [en, provs, sel, ps, act, oBase, cBase, liCmd, liTo] =
        await Promise.all([
          api.getEnhancementEnabled(),
          api.listLlmProviders(),
          api.getLlmSelection(),
          api.listPrompts(),
          api.getActivePromptId(),
          api.getOllamaBaseUrl(),
          api.getCustomBaseUrl(),
          api.getLocalcliCustomCmd(),
          api.getLocalcliTimeoutSecs(),
        ]);
      setEnabled(en);
      setProviders(provs);
      setSelection(sel);
      setPrompts(ps);
      setActivePromptId(act);
      setOllamaBaseUrl(oBase);
      setCustomBaseUrl(cBase ?? "");
      setLocalcliCustomCmd(liCmd ?? "");
      setLocalcliTimeout(liTo);
      if (sel?.provider_id === "custom") {
        setCustomModel(sel.model);
      }
      if (sel?.provider_id === "ollama") {
        refreshOllamaModels();
      }
    } catch (e) {
      console.error(e);
    }
  }

  async function refreshOllamaModels() {
    setOllamaStatus(t("enhancement.loadingModels"));
    try {
      const models = await api.listOllamaModels();
      setOllamaModels(models);
      setOllamaStatus(t("enhancement.modelsCount", { count: models.length }));
    } catch (e) {
      setOllamaModels([]);
      setOllamaStatus(t("enhancement.errorPrefix", { message: String(e) }));
    }
  }

  async function saveOllamaBaseUrl() {
    try {
      await api.setOllamaBaseUrl(ollamaBaseUrl.trim());
      setOllamaStatus(t("enhancement.urlSaved"));
      await refreshOllamaModels();
    } catch (e) {
      setOllamaStatus(t("enhancement.errorPrefix", { message: String(e) }));
    }
  }

  async function saveCustomBaseUrl() {
    try {
      await api.setCustomBaseUrl(customBaseUrl.trim());
      setStatus((s) => ({ ...s, custom: t("enhancement.urlSaved") }));
    } catch (e) {
      setStatus((s) => ({ ...s, custom: t("enhancement.errorPrefix", { message: String(e) }) }));
    }
  }

  async function selectCustomModel(model: string) {
    setCustomModel(model);
    await api.setLlmSelection("custom", model);
    setSelection({ provider_id: "custom", model });
  }

  async function toggleEnabled(v: boolean) {
    setEnabled(v);
    await api.setEnhancementEnabled(v);
  }

  async function selectProvider(providerId: string) {
    const p = providers.find((x) => x.id === providerId);
    if (!p) return;
    const model = p.default_model || "";
    setSelection({ provider_id: providerId, model });
    await api.setLlmSelection(providerId, model);
    if (providerId === "ollama") {
      await refreshOllamaModels();
    }
  }

  async function selectModel(model: string) {
    if (!selection) return;
    setSelection({ ...selection, model });
    await api.setLlmSelection(selection.provider_id, model);
  }

  async function selectPrompt(id: string) {
    setActivePromptId(id);
    await api.setActivePromptId(id);
  }

  async function saveApiKey(providerId: string) {
    const k = (apiKeyInputs[providerId] ?? "").trim();
    if (!k) return;
    setVerifying((v) => ({ ...v, [providerId]: true }));
    setStatus((s) => ({ ...s, [providerId]: t("enhancement.saveKeyInProgress") }));
    try {
      await api.setApiKey(providerId, k);
      setStatus((s) => ({ ...s, [providerId]: t("enhancement.keySavedOk") }));
      setApiKeyInputs((i) => ({ ...i, [providerId]: "" }));
      await refresh();
    } catch (e) {
      setStatus((s) => ({ ...s, [providerId]: t("enhancement.errorPrefix", { message: String(e) }) }));
    } finally {
      setVerifying((v) => ({ ...v, [providerId]: false }));
    }
  }

  async function deleteKey(providerId: string) {
    try {
      await api.deleteApiKey(providerId);
      setStatus((s) => ({ ...s, [providerId]: t("enhancement.keyDeleted") }));
      await refresh();
    } catch (e) {
      setStatus((s) => ({ ...s, [providerId]: t("enhancement.errorPrefix", { message: String(e) }) }));
    }
  }

  const currentProvider = providers.find(
    (p) => p.id === selection?.provider_id,
  );
  const isOllama = selection?.provider_id === "ollama";
  const isCustom = selection?.provider_id === "custom";
  const isLocalcli = selection?.provider_id === "localcli";
  const isLocalcliCustom = isLocalcli && selection?.model === "custom";

  async function saveLocalcliCustomCmd() {
    try {
      await api.setLocalcliCustomCmd(localcliCustomCmd.trim());
      setLocalcliStatus(t("enhancement.commandSaved"));
    } catch (e) {
      setLocalcliStatus(t("enhancement.errorPrefix", { message: String(e) }));
    }
  }

  async function saveLocalcliTimeout() {
    try {
      await api.setLocalcliTimeoutSecs(Math.max(5, Math.floor(localcliTimeout)));
      setLocalcliStatus(t("enhancement.timeoutSaved"));
    } catch (e) {
      setLocalcliStatus(t("enhancement.errorPrefix", { message: String(e) }));
    }
  }

  return (
    <Card>
      <CardHeader>
        <div className="flex items-center gap-2">
          <Sparkles className="h-4 w-4 text-muted-foreground" />
          <CardTitle className="text-base">{t("enhancement.title")}</CardTitle>
        </div>
        <CardDescription>{t("enhancement.description")}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <label className="flex items-center justify-between rounded-md border p-3">
          <div>
            <p className="font-medium">{t("enhancement.enableTitle")}</p>
            <p className="text-xs text-muted-foreground">
              {t("enhancement.enableDescription")}
            </p>
          </div>
          <input
            type="checkbox"
            checked={enabled}
            onChange={(e) => toggleEnabled(e.target.checked)}
            className="h-5 w-5"
          />
        </label>

        <div className="grid gap-2">
          <label className="text-sm font-medium">{t("enhancement.provider")}</label>
          <select
            value={selection?.provider_id ?? ""}
            onChange={(e) => selectProvider(e.target.value)}
            className="h-9 rounded-md border border-input bg-background px-2 text-sm"
          >
            <option value="">{t("enhancement.selectPlaceholder")}</option>
            {providers.map((p) => (
              <option key={p.id} value={p.id}>
                {p.label} {p.has_api_key ? t("enhancement.keyOk") : ""}
              </option>
            ))}
          </select>
        </div>

        {currentProvider && currentProvider.models.length > 0 && !isCustom && !isLocalcli && !isOllama && (
          <div className="grid gap-2">
            <label className="text-sm font-medium">{t("enhancement.model")}</label>
            <select
              value={selection?.model ?? ""}
              onChange={(e) => selectModel(e.target.value)}
              className="h-9 rounded-md border border-input bg-background px-2 text-sm"
            >
              {currentProvider.models.map((m) => (
                <option key={m} value={m}>
                  {m}
                </option>
              ))}
            </select>
            <p className="text-xs text-muted-foreground">
              {t("enhancement.endpoint")} <code>{currentProvider.endpoint}</code>
            </p>
          </div>
        )}

        {isOllama && (
          <div className="grid gap-2 rounded-md border p-3">
            <label className="text-sm font-medium">{t("enhancement.ollamaBaseUrl")}</label>
            <div className="flex gap-2">
              <input
                type="text"
                value={ollamaBaseUrl}
                onChange={(e) => setOllamaBaseUrl(e.target.value)}
                placeholder="http://localhost:11434"
                className="flex h-9 flex-1 rounded-md border border-input bg-background px-3 text-sm"
              />
              <Button size="sm" onClick={saveOllamaBaseUrl}>
                {t("enhancement.save")}
              </Button>
              <Button size="sm" variant="outline" onClick={refreshOllamaModels}>
                {t("enhancement.refreshModels")}
              </Button>
            </div>
            {ollamaStatus && (
              <p className="text-xs text-muted-foreground">{ollamaStatus}</p>
            )}
            <label className="text-sm font-medium">{t("enhancement.model")}</label>
            <select
              value={selection?.model ?? ""}
              onChange={(e) => selectModel(e.target.value)}
              className="h-9 rounded-md border border-input bg-background px-2 text-sm"
            >
              {!ollamaModels.includes(selection?.model ?? "") && selection?.model && (
                <option value={selection.model}>{selection.model}</option>
              )}
              {ollamaModels.map((m) => (
                <option key={m} value={m}>
                  {m}
                </option>
              ))}
            </select>
          </div>
        )}

        {isLocalcli && (
          <div className="grid gap-2 rounded-md border p-3">
            <p className="text-sm font-medium">{t("enhancement.localcliTemplate")}</p>
            <select
              value={selection?.model ?? "pi"}
              onChange={(e) => selectModel(e.target.value)}
              className="h-9 rounded-md border border-input bg-background px-2 text-sm"
            >
              <option value="pi">pi</option>
              <option value="claude">claude</option>
              <option value="codex">codex</option>
              <option value="custom">custom</option>
            </select>
            <p className="text-xs text-muted-foreground">
              {t("enhancement.localcliHelpA")}{" "}
              <code>powershell.exe -NoProfile -Command</code>
              {t("enhancement.localcliHelpB")}{" "}
              <code>$env:PARLA_SYSTEM_PROMPT</code>,{" "}
              <code>$env:PARLA_USER_PROMPT</code>,{" "}
              <code>$env:PARLA_FULL_PROMPT</code>.
            </p>

            {isLocalcliCustom && (
              <div className="grid gap-2">
                <label className="text-sm font-medium">{t("enhancement.customCommandLabel")}</label>
                <textarea
                  value={localcliCustomCmd}
                  onChange={(e) => setLocalcliCustomCmd(e.target.value)}
                  placeholder="& mon-cli -p $env:PARLA_FULL_PROMPT"
                  className="min-h-[80px] rounded-md border border-input bg-background px-3 py-2 text-sm font-mono"
                />
                <Button size="sm" onClick={saveLocalcliCustomCmd}>
                  {t("enhancement.saveCommand")}
                </Button>
              </div>
            )}

            <div className="grid grid-cols-[1fr_auto] items-end gap-2">
              <div className="grid gap-2">
                <label className="text-sm font-medium">{t("enhancement.timeoutLabel")}</label>
                <input
                  type="number"
                  min={5}
                  max={300}
                  value={localcliTimeout}
                  onChange={(e) => setLocalcliTimeout(Number(e.target.value) || 45)}
                  className="h-9 rounded-md border border-input bg-background px-3 text-sm"
                />
              </div>
              <Button size="sm" onClick={saveLocalcliTimeout}>
                {t("enhancement.saveTimeout")}
              </Button>
            </div>

            {localcliStatus && (
              <p className="text-xs text-muted-foreground">{localcliStatus}</p>
            )}
          </div>
        )}

        {isCustom && (
          <div className="grid gap-2 rounded-md border p-3">
            <label className="text-sm font-medium">{t("enhancement.customBaseUrlLabel")}</label>
            <div className="flex gap-2">
              <input
                type="text"
                value={customBaseUrl}
                onChange={(e) => setCustomBaseUrl(e.target.value)}
                placeholder="https://my-llm.example.com/v1"
                className="flex h-9 flex-1 rounded-md border border-input bg-background px-3 text-sm"
              />
              <Button size="sm" onClick={saveCustomBaseUrl}>
                {t("enhancement.saveUrl")}
              </Button>
            </div>
            <p className="text-xs text-muted-foreground">
              {t("enhancement.customBaseUrlHelp")}
              <code>/chat/completions</code>
              {t("enhancement.customBaseUrlHelpEnd")}
            </p>
            <label className="text-sm font-medium">{t("enhancement.model")}</label>
            <input
              type="text"
              value={customModel}
              onChange={(e) => setCustomModel(e.target.value)}
              onBlur={(e) => selectCustomModel(e.target.value)}
              placeholder={t("enhancement.customModelPlaceholder")}
              className="flex h-9 rounded-md border border-input bg-background px-3 text-sm"
            />
            {status.custom && (
              <p className="text-xs text-muted-foreground">{status.custom}</p>
            )}
          </div>
        )}

        {currentProvider && currentProvider.requires_api_key && (
          <div
            className={cn(
              "rounded-lg border p-3",
              currentProvider.has_api_key && "border-green-500/40 bg-green-500/5",
            )}
          >
            <div className="flex items-center gap-2">
              <Key className="h-4 w-4 text-muted-foreground" />
              <p className="font-medium">
                {t("enhancement.apiKeyLabel", { provider: currentProvider.label })}
              </p>
              {currentProvider.has_api_key && (
                <span className="flex items-center gap-1 text-[11px] text-green-600 dark:text-green-400">
                  <Check className="h-3 w-3" /> {t("enhancement.configured")}
                </span>
              )}
            </div>
            <div className="mt-3 grid grid-cols-[1fr_auto_auto] gap-2">
              <input
                type="password"
                placeholder={
                  currentProvider.has_api_key
                    ? t("enhancement.apiKeySavedPlaceholder")
                    : t("enhancement.apiKey")
                }
                value={apiKeyInputs[currentProvider.id] ?? ""}
                onChange={(e) =>
                  setApiKeyInputs((i) => ({
                    ...i,
                    [currentProvider.id]: e.target.value,
                  }))
                }
                className="flex h-9 rounded-md border border-input bg-background px-3 text-sm shadow-sm"
                autoComplete="off"
              />
              <Button
                size="sm"
                onClick={() => saveApiKey(currentProvider.id)}
                disabled={verifying[currentProvider.id]}
              >
                {verifying[currentProvider.id] ? (
                  <Loader2 className="animate-spin" />
                ) : null}
                {t("enhancement.saveKey")}
              </Button>
              {currentProvider.has_api_key && (
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => deleteKey(currentProvider.id)}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </Button>
              )}
            </div>
            {status[currentProvider.id] && (
              <p
                className={cn(
                  "mt-2 text-xs",
                  status[currentProvider.id].startsWith(t("common.error"))
                    ? "text-destructive"
                    : "text-muted-foreground",
                )}
              >
                {status[currentProvider.id]}
              </p>
            )}
          </div>
        )}

        <div className="grid gap-2">
          <label className="text-sm font-medium">{t("enhancement.activePrompt")}</label>
          <select
            value={activePromptId ?? ""}
            onChange={(e) => selectPrompt(e.target.value)}
            className="h-9 rounded-md border border-input bg-background px-2 text-sm"
          >
            {prompts.map((p) => (
              <option key={p.id} value={p.id}>
                {promptTitle(t, p)} {p.is_predefined ? t("enhancement.predefined") : ""}
              </option>
            ))}
          </select>
          {(() => {
            const active = prompts.find((p) => p.id === activePromptId);
            if (!active) return null;
            const desc = promptDescription(t, active);
            return desc ? (
              <p className="text-xs text-muted-foreground">{desc}</p>
            ) : null;
          })()}
        </div>

        <EnhancementScreenContext />

        <PromptEditor
          prompts={prompts}
          activeId={activePromptId}
          onChange={refresh}
        />
      </CardContent>
    </Card>
  );
}
