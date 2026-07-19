import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Eye,
  Pencil,
  Plus,
  Star,
  Trash2,
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
import {
  Sheet,
  SheetClose,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { cn, powerShortcutLabel } from "@/lib/utils";
import {
  api,
  type CustomPrompt,
  type DetectionPreview,
  type LLMProviderInfo,
  type PowerModeConfig,
} from "@/lib/tauri";

function emptyConfig(defaultName: string): PowerModeConfig {
  return {
    id: "",
    name: defaultName,
    emoji: "*",
    app_triggers: [],
    url_triggers: [],
    is_enhancement_enabled: false,
    use_screen_capture: null,
    selected_prompt_id: null,
    selected_llm_provider: null,
    selected_llm_model: null,
    transcription_kind: null,
    whisper_model_id: null,
    cloud_provider: null,
    cloud_model: null,
    parakeet_model_id: null,
    language: null,
    auto_send_key: "none",
    is_enabled: true,
    is_default: false,
  };
}

export function PowerModePanel() {
  const { t } = useTranslation();
  const [configs, setConfigs] = useState<PowerModeConfig[]>([]);
  const [autoRestore, setAutoRestore] = useState(true);
  const [editing, setEditing] = useState<PowerModeConfig | null>(null);
  const [isNew, setIsNew] = useState(false);
  const [prompts, setPrompts] = useState<CustomPrompt[]>([]);
  const [providers, setProviders] = useState<LLMProviderInfo[]>([]);
  const [preview, setPreview] = useState<DetectionPreview | null>(null);

  useEffect(() => {
    refresh();
  }, []);

  async function refresh() {
    try {
      const [cs, ar, ps, pr] = await Promise.all([
        api.listPowerConfigs(),
        api.getPowerAutoRestore(),
        api.listPrompts(),
        api.listLlmProviders(),
      ]);
      setConfigs(cs);
      setAutoRestore(ar);
      setPrompts(ps);
      setProviders(pr);
    } catch (e) {
      console.error(e);
    }
  }

  async function runPreview() {
    try {
      const p = await api.powerModePreview();
      setPreview(p);
    } catch (e) {
      setPreview(null);
      console.error(e);
    }
  }

  function startNew() {
    setEditing(emptyConfig(t("powerMode.defaultName")));
    setIsNew(true);
  }

  function startEdit(c: PowerModeConfig) {
    setEditing({ ...c });
    setIsNew(false);
  }

  async function save() {
    if (!editing) return;
    try {
      if (isNew) {
        await api.addPowerConfig(editing);
      } else {
        await api.updatePowerConfig(editing);
      }
      setEditing(null);
      setIsNew(false);
      await refresh();
    } catch (e) {
      console.error(e);
    }
  }

  async function remove(c: PowerModeConfig) {
    if (!confirm(t("powerMode.confirmDelete", { name: c.name }))) return;
    await api.deletePowerConfig(c.id);
    await refresh();
  }

  async function toggleAutoRestore(v: boolean) {
    setAutoRestore(v);
    await api.setPowerAutoRestore(v);
  }

  return (
    <Card>
      <CardHeader>
        <div className="flex items-center gap-2">
          <Zap className="h-4 w-4 text-muted-foreground" />
          <CardTitle className="text-base">{t("powerMode.title")}</CardTitle>
        </div>
        <CardDescription>{t("powerMode.description")}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <label className="flex items-center justify-between rounded-md border p-3">
          <div>
            <p className="font-medium">{t("powerMode.autoRestoreTitle")}</p>
            <p className="text-xs text-muted-foreground">
              {t("powerMode.autoRestoreDescription")}
            </p>
          </div>
          <input
            type="checkbox"
            checked={autoRestore}
            onChange={(e) => toggleAutoRestore(e.target.checked)}
            className="h-5 w-5"
          />
        </label>

        <div className="flex items-center justify-between">
          <p className="text-sm font-medium">{t("powerMode.configurations")}</p>
          <div className="flex gap-2">
            <Button size="sm" variant="outline" onClick={runPreview}>
              <Eye className="h-3.5 w-3.5" />
              {t("powerMode.previewDetection")}
            </Button>
            {!editing && (
              <Button size="sm" onClick={startNew}>
                <Plus className="h-3.5 w-3.5" />
                {t("powerMode.newConfig")}
              </Button>
            )}
          </div>
        </div>

        {preview && (
          <div className="rounded-md border border-dashed bg-muted/30 p-3 text-xs">
            <p>
              {t("powerMode.previewWindow")} <code>{preview.active.exe_name}</code> - {preview.active.title}
            </p>
            {preview.url && (
              <p>
                {t("powerMode.previewUrl")} <code>{preview.url}</code>
              </p>
            )}
            <p>
              {t("powerMode.matchedConfig")}{" "}
              {preview.matched_config_name ? (
                <strong>{preview.matched_config_name}</strong>
              ) : (
                <span className="text-muted-foreground">{t("powerMode.noMatch")}</span>
              )}
            </p>
          </div>
        )}

        <ul className="grid gap-1.5">
            {configs.map((c) => {
              // Index parmi les profils actives, dans l'ordre stocke : c'est
              // ce que le raccourci Alt+chiffre cible (cote backend :
              // session::enabled_configs). -1 si desactive.
              const enabledIdx = c.is_enabled
                ? configs.filter((x) => x.is_enabled).indexOf(c)
                : -1;
              const shortcut = powerShortcutLabel(enabledIdx);
              return (
              <li
                key={c.id}
                className={cn(
                  "flex items-center justify-between rounded-md border p-2",
                  !c.is_enabled && "opacity-60",
                )}
              >
                <div className="flex min-w-0 items-center gap-2">
                  <span className="text-lg">{c.emoji}</span>
                  <div className="min-w-0">
                    <p className="truncate text-sm font-medium">
                      {c.name}
                      {c.is_default && (
                        <Star className="ml-1 inline h-3 w-3 fill-current text-amber-500" />
                      )}
                    </p>
                    <p className="truncate text-xs text-muted-foreground">
                      {c.app_triggers.length}
                      {c.app_triggers.length > 1
                        ? t("powerMode.appsPlural")
                        : t("powerMode.appsSingular")}
                      ,{" "}
                      {c.url_triggers.length}
                      {c.url_triggers.length > 1
                        ? t("powerMode.urlsPlural")
                        : t("powerMode.urlsSingular")}
                    </p>
                  </div>
                </div>
                <div className="flex shrink-0 items-center gap-1">
                  {shortcut && (
                    <kbd
                      className="rounded border bg-muted px-1.5 py-0.5 font-mono text-[10px] font-medium text-muted-foreground"
                      title={t("powerMode.shortcutHint")}
                    >
                      {shortcut}
                    </kbd>
                  )}
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => startEdit(c)}
                    title={t("powerMode.editTooltip")}
                  >
                    <Pencil className="h-3.5 w-3.5" />
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => remove(c)}
                    title={t("powerMode.deleteTooltip")}
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                  </Button>
                </div>
              </li>
              );
            })}
            {configs.length === 0 && (
              <p className="text-xs text-muted-foreground">
                {t("powerMode.emptyList")}
              </p>
            )}
          </ul>

        <Sheet
          open={editing !== null}
          onOpenChange={(open) => {
            if (!open) {
              setEditing(null);
              setIsNew(false);
            }
          }}
        >
          <SheetContent
            side="right"
            className="w-[400px] overflow-y-auto sm:max-w-[400px]"
          >
            <SheetHeader>
              <SheetTitle>
                {isNew
                  ? t("powerMode.newConfigTitle")
                  : t("powerMode.editConfigTitle")}
              </SheetTitle>
              <SheetDescription>
                {t("powerMode.sheetDescription")}
              </SheetDescription>
            </SheetHeader>

            {editing && (
              <div className="mt-4 space-y-3 pb-24">
                <ConfigEditor
                  config={editing}
                  prompts={prompts}
                  providers={providers}
                  onChange={setEditing}
                  onCancel={() => {
                    setEditing(null);
                    setIsNew(false);
                  }}
                  onSave={save}
                />
              </div>
            )}

            <SheetFooter className="absolute bottom-0 left-0 right-0 border-t bg-background p-4">
              <SheetClose asChild>
                <Button variant="ghost">{t("common.cancel")}</Button>
              </SheetClose>
              <Button onClick={save}>{t("common.save")}</Button>
            </SheetFooter>
          </SheetContent>
        </Sheet>
      </CardContent>
    </Card>
  );
}

function ConfigEditor({
  config,
  prompts,
  providers,
  onChange,
}: {
  config: PowerModeConfig;
  prompts: CustomPrompt[];
  providers: LLMProviderInfo[];
  onChange: (c: PowerModeConfig) => void;
  onCancel: () => void;
  onSave: () => void;
}) {
  const { t } = useTranslation();

  function set<K extends keyof PowerModeConfig>(key: K, value: PowerModeConfig[K]) {
    onChange({ ...config, [key]: value });
  }

  function addApp() {
    const exe = prompt(t("powerMode.promptAppName"));
    if (!exe) return;
    const trimmed = exe.trim().toLowerCase();
    if (!trimmed) return;
    onChange({
      ...config,
      app_triggers: [
        ...config.app_triggers,
        {
          id: crypto.randomUUID(),
          exe_name: trimmed,
          app_name: trimmed,
        },
      ],
    });
  }

  function removeApp(id: string) {
    onChange({
      ...config,
      app_triggers: config.app_triggers.filter((tr) => tr.id !== id),
    });
  }

  function addUrl() {
    const url = prompt(t("powerMode.promptUrl"));
    if (!url) return;
    const trimmed = url.trim();
    if (!trimmed) return;
    onChange({
      ...config,
      url_triggers: [
        ...config.url_triggers,
        { id: crypto.randomUUID(), url: trimmed },
      ],
    });
  }

  function removeUrl(id: string) {
    onChange({
      ...config,
      url_triggers: config.url_triggers.filter((tr) => tr.id !== id),
    });
  }

  const currentProvider = providers.find((p) => p.id === config.selected_llm_provider);

  return (
    <div className="grid gap-3 rounded-md border bg-muted/30 p-3">
      <div className="grid grid-cols-[auto_1fr_auto] items-center gap-2">
        <input
          type="text"
          value={config.emoji}
          onChange={(e) => set("emoji", e.target.value.slice(0, 4))}
          placeholder="*"
          className="h-9 w-14 rounded-md border border-input bg-background px-2 text-center text-lg"
        />
        <input
          type="text"
          value={config.name}
          onChange={(e) => set("name", e.target.value)}
          placeholder={t("powerMode.namePlaceholder")}
          className="h-9 rounded-md border border-input bg-background px-2 text-sm"
        />
        <label className="flex items-center gap-1 text-xs">
          <input
            type="checkbox"
            checked={config.is_default}
            onChange={(e) => set("is_default", e.target.checked)}
          />
          {t("powerMode.defaultLabel")}
        </label>
      </div>

      <label className="flex items-center gap-2 text-xs">
        <input
          type="checkbox"
          checked={config.is_enabled}
          onChange={(e) => set("is_enabled", e.target.checked)}
        />
        {t("powerMode.enabledLabel")}
      </label>

      <div className="grid gap-2">
        <div className="flex items-center justify-between">
          <p className="text-xs font-medium">{t("powerMode.apps")}</p>
          <Button size="sm" variant="outline" onClick={addApp}>
            <Plus className="h-3 w-3" /> {t("powerMode.addTrigger")}
          </Button>
        </div>
        <ul className="grid gap-1">
          {config.app_triggers.map((tr) => (
            <li key={tr.id} className="flex items-center justify-between rounded border p-1.5 text-xs">
              <code>{tr.exe_name}</code>
              <Button size="sm" variant="ghost" onClick={() => removeApp(tr.id)}>
                <Trash2 className="h-3 w-3" />
              </Button>
            </li>
          ))}
        </ul>
      </div>

      <div className="grid gap-2">
        <div className="flex items-center justify-between">
          <p className="text-xs font-medium">{t("powerMode.urls")}</p>
          <Button size="sm" variant="outline" onClick={addUrl}>
            <Plus className="h-3 w-3" /> {t("powerMode.addTrigger")}
          </Button>
        </div>
        <ul className="grid gap-1">
          {config.url_triggers.map((tr) => (
            <li key={tr.id} className="flex items-center justify-between rounded border p-1.5 text-xs">
              <code>{tr.url}</code>
              <Button size="sm" variant="ghost" onClick={() => removeUrl(tr.id)}>
                <Trash2 className="h-3 w-3" />
              </Button>
            </li>
          ))}
        </ul>
      </div>

      <fieldset className="grid gap-2 rounded-md border p-2">
        <legend className="px-1 text-xs font-medium">{t("powerMode.enhancementFieldset")}</legend>
        <label className="flex items-center gap-2 text-xs">
          <input
            type="checkbox"
            checked={config.is_enhancement_enabled}
            onChange={(e) => set("is_enhancement_enabled", e.target.checked)}
          />
          {t("powerMode.enableEnhancement")}
        </label>
        <label className="flex items-center gap-2 text-xs">
          <input
            type="checkbox"
            checked={config.use_screen_capture === true}
            ref={(el) => {
              if (el) el.indeterminate = config.use_screen_capture === null;
            }}
            onChange={(e) => {
              // Tri-state : coche/decoche/neutre.
              if (config.use_screen_capture === null) {
                set("use_screen_capture", true);
              } else if (config.use_screen_capture) {
                set("use_screen_capture", false);
              } else {
                set("use_screen_capture", null);
              }
              e.preventDefault();
            }}
          />
          {t("powerMode.screenContextPrefix")}{" "}
          {config.use_screen_capture === null
            ? t("powerMode.screenContextUnchanged")
            : config.use_screen_capture
              ? t("powerMode.screenContextActive")
              : t("powerMode.screenContextInactive")}
        </label>
        <label className="grid gap-1 text-xs">
          {t("powerMode.promptSelect")}
          <select
            value={config.selected_prompt_id ?? ""}
            onChange={(e) =>
              set("selected_prompt_id", e.target.value || null)
            }
            className="h-8 rounded-md border border-input bg-background px-2"
          >
            <option value="">{t("powerMode.unchangedOption")}</option>
            {prompts.map((p) => (
              <option key={p.id} value={p.id}>
                {p.title}
              </option>
            ))}
          </select>
        </label>
        <div className="grid grid-cols-2 gap-2">
          <label className="grid gap-1 text-xs">
            {t("powerMode.providerLlm")}
            <select
              value={config.selected_llm_provider ?? ""}
              onChange={(e) => {
                const id = e.target.value || null;
                set("selected_llm_provider", id);
                if (id) {
                  const p = providers.find((x) => x.id === id);
                  set("selected_llm_model", p?.default_model ?? null);
                }
              }}
              className="h-8 rounded-md border border-input bg-background px-2"
            >
              <option value="">{t("powerMode.unchangedOption")}</option>
              {providers.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.label}
                </option>
              ))}
            </select>
          </label>
          <label className="grid gap-1 text-xs">
            {t("powerMode.modelLlm")}
            <input
              type="text"
              value={config.selected_llm_model ?? ""}
              onChange={(e) =>
                set("selected_llm_model", e.target.value || null)
              }
              placeholder={currentProvider?.default_model ?? "-"}
              className="h-8 rounded-md border border-input bg-background px-2"
            />
          </label>
        </div>
      </fieldset>

      <fieldset className="grid gap-2 rounded-md border p-2">
        <legend className="px-1 text-xs font-medium">{t("powerMode.sourceFieldset")}</legend>
        <label className="grid gap-1 text-xs">
          {t("powerMode.type")}
          <select
            value={config.transcription_kind ?? ""}
            onChange={(e) =>
              set("transcription_kind", e.target.value || null)
            }
            className="h-8 rounded-md border border-input bg-background px-2"
          >
            <option value="">{t("powerMode.unchangedOption")}</option>
            <option value="local">{t("powerMode.typeLocal")}</option>
            <option value="parakeet">{t("powerMode.typeParakeet")}</option>
            <option value="cloud">{t("powerMode.typeCloud")}</option>
          </select>
        </label>
        {config.transcription_kind === "local" && (
          <label className="grid gap-1 text-xs">
            {t("powerMode.whisperModelId")}
            <input
              type="text"
              value={config.whisper_model_id ?? ""}
              onChange={(e) =>
                set("whisper_model_id", e.target.value || null)
              }
              placeholder="ggml-base.en"
              className="h-8 rounded-md border border-input bg-background px-2"
            />
          </label>
        )}
        {config.transcription_kind === "parakeet" && (
          <label className="grid gap-1 text-xs">
            {t("powerMode.parakeetModelId")}
            <input
              type="text"
              value={config.parakeet_model_id ?? ""}
              onChange={(e) =>
                set("parakeet_model_id", e.target.value || null)
              }
              placeholder="parakeet-tdt-0.6b-v3-int8"
              className="h-8 rounded-md border border-input bg-background px-2"
            />
          </label>
        )}
        {config.transcription_kind === "cloud" && (
          <div className="grid grid-cols-2 gap-2">
            <label className="grid gap-1 text-xs">
              {t("powerMode.cloudProvider")}
              <input
                type="text"
                value={config.cloud_provider ?? ""}
                onChange={(e) =>
                  set("cloud_provider", e.target.value || null)
                }
                className="h-8 rounded-md border border-input bg-background px-2"
              />
            </label>
            <label className="grid gap-1 text-xs">
              {t("powerMode.cloudModel")}
              <input
                type="text"
                value={config.cloud_model ?? ""}
                onChange={(e) =>
                  set("cloud_model", e.target.value || null)
                }
                className="h-8 rounded-md border border-input bg-background px-2"
              />
            </label>
          </div>
        )}
        <label className="grid gap-1 text-xs">
          {t("powerMode.language")}
          <input
            type="text"
            value={config.language ?? ""}
            onChange={(e) => set("language", e.target.value || null)}
            placeholder={t("powerMode.languagePlaceholder")}
            className="h-8 rounded-md border border-input bg-background px-2"
          />
        </label>
      </fieldset>
    </div>
  );
}
