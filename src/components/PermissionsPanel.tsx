// Page Permissions.
//
// Reference VoiceInk Views/PermissionsView.swift : VStack de PermissionCards
// (icone + titre + description + status dot + bouton action + InfoTip).
//
// Sur Windows les permissions concernees sont : microphone, OCR
// (Windows.Media.Ocr), auto-demarrage, et le hook clavier global (toujours
// ok sous Win32).

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Check,
  ExternalLink,
  Keyboard,
  Languages,
  Loader2,
  Mic,
  Power,
  RefreshCw,
  X,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { InfoTip } from "@/components/ui/info-tip";
import { api, type PermissionState, type PermissionStatus } from "@/lib/tauri";
import { cn } from "@/lib/utils";

export function PermissionsPanel() {
  const { t } = useTranslation();
  const [status, setStatus] = useState<PermissionStatus | null>(null);
  const [loading, setLoading] = useState(false);

  async function refresh() {
    setLoading(true);
    try {
      const s = await api.checkPermissions();
      setStatus(s);
    } catch (e) {
      console.error(e);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  async function toggleAutostart(enabled: boolean) {
    try {
      await api.setAutostartEnabled(enabled);
      await refresh();
    } catch (e) {
      console.error(e);
    }
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <p className="text-sm text-muted-foreground">
          {t("permissions.intro")}
        </p>
        <Button size="sm" variant="outline" onClick={refresh} disabled={loading}>
          {loading ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <RefreshCw className="h-3.5 w-3.5" />
          )}
          {t("permissions.refresh")}
        </Button>
      </div>

      <PermissionCard
        icon={Mic}
        title={t("permissions.microphoneTitle")}
        description={t("permissions.microphoneDescription")}
        state={status?.microphone}
        action={
          <Button size="sm" onClick={() => api.openPrivacyMicrophone()}>
            <ExternalLink className="h-3.5 w-3.5" />
            {t("permissions.microphoneAction")}
          </Button>
        }
        tip={<InfoTip>{t("permissions.microphoneTip")}</InfoTip>}
      />

      <PermissionCard
        icon={Languages}
        title={t("permissions.ocrTitle")}
        description={t("permissions.ocrDescription")}
        state={status?.ocr}
        action={
          <Button size="sm" onClick={() => api.openLanguageSettings()}>
            <ExternalLink className="h-3.5 w-3.5" />
            {t("permissions.ocrAction")}
          </Button>
        }
        tip={<InfoTip>{t("permissions.ocrTip")}</InfoTip>}
      />

      <PermissionCard
        icon={Power}
        title={t("permissions.autostartTitle")}
        description={t("permissions.autostartDescription")}
        state={status?.autostart}
        action={
          status?.autostart ? (
            <Button
              size="sm"
              variant={status.autostart.ok ? "ghost" : "default"}
              onClick={() => toggleAutostart(!status.autostart.ok)}
            >
              {status.autostart.ok
                ? t("permissions.autostartDeactivate")
                : t("permissions.autostartActivate")}
            </Button>
          ) : null
        }
      />

      <PermissionCard
        icon={Keyboard}
        title={t("permissions.hotkeyTitle")}
        description={t("permissions.hotkeyDescription")}
        state={status?.hotkey}
        tip={<InfoTip>{t("permissions.hotkeyTip")}</InfoTip>}
      />
    </div>
  );
}

function PermissionCard({
  icon: Icon,
  title,
  description,
  state,
  action,
  tip,
}: {
  icon: LucideIcon;
  title: string;
  description: string;
  state?: PermissionState;
  action?: React.ReactNode;
  tip?: React.ReactNode;
}) {
  const { t } = useTranslation();
  const ok = state?.ok ?? false;
  return (
    <Card>
      <CardContent className="flex items-start gap-3 p-4">
        <div
          className={cn(
            "mt-0.5 flex h-9 w-9 shrink-0 items-center justify-center rounded-full",
            ok
              ? "bg-green-500/10 text-green-600 dark:text-green-400"
              : "bg-amber-500/10 text-amber-600 dark:text-amber-400",
          )}
        >
          <Icon className="h-4 w-4" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <p className="font-medium">{title}</p>
            {tip}
          </div>
          <p className="text-xs text-muted-foreground">{description}</p>
          {state && (
            <p className="mt-1 flex items-center gap-1 text-xs">
              {ok ? (
                <Check className="h-3 w-3 text-green-500" />
              ) : (
                <X className="h-3 w-3 text-amber-500" />
              )}
              <span className={cn(ok ? "text-green-600" : "text-amber-600")}>
                {t(state.label_key, state.label_args ?? undefined)}
              </span>
            </p>
          )}
          {state?.hint_key && !state.ok && (
            <p className="mt-1 text-[11px] text-muted-foreground">
              {t(state.hint_key)}
            </p>
          )}
          {state?.diagnostic && !state.ok && (
            <p className="mt-1 text-[10px] text-muted-foreground/70 font-mono break-all">
              {state.diagnostic}
            </p>
          )}
        </div>
        {action && <div className="shrink-0">{action}</div>}
      </CardContent>
    </Card>
  );
}
