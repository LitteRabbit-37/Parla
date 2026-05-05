// Editeur de prompts custom.
//
// Reference VoiceInk PromptEditorView + SlidingPanel 400pt.
// shadcn pur : Sheet depuis la droite.

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Copy, Pencil, Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
  SheetFooter,
  SheetClose,
} from "@/components/ui/sheet";
import { api, type CustomPrompt } from "@/lib/tauri";
import { promptDescription, promptTitle } from "@/lib/promptLabels";
import { cn } from "@/lib/utils";

type Props = {
  prompts: CustomPrompt[];
  activeId: string | null;
  onChange: () => void | Promise<void>;
};

function emptyPrompt(defaultTitle: string): CustomPrompt {
  return {
    id: "",
    title: defaultTitle,
    prompt_text: "",
    icon: "pencil",
    description: null,
    is_predefined: false,
    trigger_words: [],
    use_system_instructions: true,
  };
}

export function PromptEditor({ prompts, activeId, onChange }: Props) {
  const { t } = useTranslation();
  const [templates, setTemplates] = useState<CustomPrompt[]>([]);
  const [editing, setEditing] = useState<CustomPrompt | null>(null);
  const [isNew, setIsNew] = useState(false);
  const [status, setStatus] = useState("");

  useEffect(() => {
    api
      .listExtraTemplates()
      .then(setTemplates)
      .catch((e) => console.error(e));
  }, []);

  function startNew() {
    setEditing(emptyPrompt(t("promptEditor.newPromptDefaultTitle")));
    setIsNew(true);
    setStatus("");
  }

  function startFromTemplate(t: CustomPrompt) {
    setEditing({ ...t, id: "" });
    setIsNew(true);
    setStatus("");
  }

  function startEdit(p: CustomPrompt) {
    setEditing({ ...p });
    setIsNew(false);
    setStatus("");
  }

  function cancel() {
    setEditing(null);
    setIsNew(false);
    setStatus("");
  }

  async function save() {
    if (!editing) return;
    try {
      if (isNew) {
        await api.addPrompt(editing);
      } else {
        await api.updatePrompt(editing);
      }
      setEditing(null);
      setIsNew(false);
      setStatus(t("promptEditor.saved"));
      await onChange();
    } catch (e) {
      setStatus(t("promptEditor.errorPrefix", { message: String(e) }));
    }
  }

  async function remove(p: CustomPrompt) {
    if (p.is_predefined) return;
    if (!confirm(t("promptEditor.confirmDelete", { name: p.title }))) return;
    try {
      await api.deletePrompt(p.id);
      await onChange();
    } catch (e) {
      setStatus(t("promptEditor.errorPrefix", { message: String(e) }));
    }
  }

  return (
    <div className="grid gap-3 rounded-md border p-3">
      <div className="flex items-center justify-between">
        <p className="text-sm font-medium">{t("promptEditor.title")}</p>
        <div className="flex gap-2">
          {templates.length > 0 && (
            <select
              onChange={(e) => {
                const tpl = templates.find((x) => x.title === e.target.value);
                if (tpl) startFromTemplate(tpl);
                e.target.value = "";
              }}
              defaultValue=""
              className="h-8 rounded-md border border-input bg-background px-2 text-xs"
            >
              <option value="" disabled>
                {t("promptEditor.fromTemplate")}
              </option>
              {templates.map((tpl) => (
                <option key={tpl.title} value={tpl.title}>
                  {tpl.title}
                </option>
              ))}
            </select>
          )}
          <Button size="sm" onClick={startNew}>
            <Plus className="h-3.5 w-3.5" />
            {t("promptEditor.newPrompt")}
          </Button>
        </div>
      </div>

      <ul className="grid gap-1.5">
        {prompts.map((p) => (
          <li
            key={p.id}
            className={cn(
              "flex items-center justify-between rounded-md border p-2",
              p.id === activeId && "border-primary/60 bg-primary/5",
            )}
          >
            <div className="min-w-0">
              <p className="truncate text-sm font-medium">
                {promptTitle(t, p)}
                {p.is_predefined && (
                  <span className="ml-2 text-[10px] text-muted-foreground">
                    {t("promptEditor.predefined")}
                  </span>
                )}
              </p>
              {(() => {
                const desc = promptDescription(t, p);
                return desc ? (
                  <p className="truncate text-xs text-muted-foreground">
                    {desc}
                  </p>
                ) : null;
              })()}
            </div>
            <div className="flex shrink-0 gap-1">
              <Button
                size="sm"
                variant="ghost"
                onClick={() => startEdit(p)}
                title={t("promptEditor.editTooltip")}
              >
                <Pencil className="h-3.5 w-3.5" />
              </Button>
              <Button
                size="sm"
                variant="ghost"
                onClick={() =>
                  startFromTemplate({
                    ...p,
                    title: `${p.title} ${t("promptEditor.duplicateSuffix")}`,
                  })
                }
                title={t("promptEditor.duplicateTooltip")}
              >
                <Copy className="h-3.5 w-3.5" />
              </Button>
              {!p.is_predefined && (
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => remove(p)}
                  title={t("promptEditor.deleteTooltip")}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </Button>
              )}
            </div>
          </li>
        ))}
      </ul>

      <Sheet
        open={editing !== null}
        onOpenChange={(open) => {
          if (!open) cancel();
        }}
      >
        <SheetContent side="right" className="w-[400px] sm:max-w-[400px]">
          <SheetHeader>
            <SheetTitle>
              {isNew
                ? t("promptEditor.newPromptTitle")
                : t("promptEditor.editPromptTitle")}
            </SheetTitle>
            <SheetDescription>
              {editing?.is_predefined
                ? t("promptEditor.predefinedDescription")
                : t("promptEditor.customDescription")}
            </SheetDescription>
          </SheetHeader>

          {editing && (
            <div className="mt-4 grid gap-3 overflow-y-auto pb-20 pr-1">
              <div className="grid gap-1">
                <label className="text-xs font-medium">{t("promptEditor.title_field")}</label>
                <input
                  type="text"
                  value={editing.title}
                  onChange={(e) =>
                    setEditing({ ...editing, title: e.target.value })
                  }
                  className="h-9 rounded-md border border-input bg-background px-3 text-sm"
                />
              </div>
              <div className="grid gap-1">
                <label className="text-xs font-medium">{t("promptEditor.description_field")}</label>
                <input
                  type="text"
                  value={editing.description ?? ""}
                  onChange={(e) =>
                    setEditing({
                      ...editing,
                      description: e.target.value || null,
                    })
                  }
                  className="h-9 rounded-md border border-input bg-background px-3 text-sm"
                />
              </div>
              <label className="flex items-start gap-2 rounded-md border p-2 text-xs">
                <input
                  type="checkbox"
                  className="mt-0.5"
                  checked={editing.use_system_instructions}
                  onChange={(e) =>
                    setEditing({
                      ...editing,
                      use_system_instructions: e.target.checked,
                    })
                  }
                />
                <span>
                  <span className="font-medium">
                    {t("promptEditor.injectWrapper")}
                  </span>
                  <br />
                  <span className="text-muted-foreground">
                    {t("promptEditor.injectWrapperHelp")}
                  </span>
                </span>
              </label>
              <div className="grid gap-1">
                <label className="text-xs font-medium">{t("promptEditor.content")}</label>
                <textarea
                  value={editing.prompt_text}
                  onChange={(e) =>
                    setEditing({ ...editing, prompt_text: e.target.value })
                  }
                  className="min-h-[220px] rounded-md border border-input bg-background px-3 py-2 font-mono text-xs"
                />
              </div>
              <div className="grid gap-1">
                <label className="text-xs font-medium">
                  {t("promptEditor.triggerWords")}
                </label>
                <input
                  type="text"
                  placeholder={t("promptEditor.triggerWordsPlaceholder")}
                  value={editing.trigger_words.join(", ")}
                  onChange={(e) =>
                    setEditing({
                      ...editing,
                      trigger_words: e.target.value
                        .split(",")
                        .map((s) => s.trim())
                        .filter(Boolean),
                    })
                  }
                  className="h-9 rounded-md border border-input bg-background px-3 text-sm"
                />
                <p className="text-[10px] text-muted-foreground">
                  {t("promptEditor.triggerWordsHelp")}
                </p>
              </div>
              {status && (
                <p className="text-xs text-muted-foreground">{status}</p>
              )}
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
    </div>
  );
}
