// Onboarding : flow 5 etapes au premier demarrage.
//
// Reference VoiceInk Views/Onboarding/OnboardingView.swift +
// OnboardingPermissionsView.swift : plein ecran noir avec accent glow,
// etape 1 welcome + etapes 2-5 permissions (microphone / mic selection
// / accessibility / screen recording / keyboard shortcut). Sur Parla
// on adapte a Windows : welcome + microphone + langue OCR + autostart
// + hotkey.

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  ArrowRight,
  Check,
  Keyboard,
  Languages,
  Mic,
  Power,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { api, type PermissionStatus } from "@/lib/tauri";
import { cn } from "@/lib/utils";

type Step = "welcome" | "microphone" | "ocr" | "autostart" | "hotkey";

const ORDER: Step[] = ["welcome", "microphone", "ocr", "autostart", "hotkey"];

export function Onboarding({ onDone }: { onDone: () => void }) {
  const { t } = useTranslation();
  const [step, setStep] = useState<Step>("welcome");
  const [perms, setPerms] = useState<PermissionStatus | null>(null);

  useEffect(() => {
    refresh();
  }, [step]);

  async function refresh() {
    try {
      const p = await api.checkPermissions();
      setPerms(p);
    } catch (e) {
      console.error(e);
    }
  }

  const currentIndex = ORDER.indexOf(step);
  function next() {
    const idx = ORDER.indexOf(step);
    if (idx < ORDER.length - 1) {
      setStep(ORDER[idx + 1]);
    } else {
      finish();
    }
  }
  function back() {
    const idx = ORDER.indexOf(step);
    if (idx > 0) setStep(ORDER[idx - 1]);
  }
  async function finish() {
    try {
      await api.setOnboardingCompleted(true);
    } catch (e) {
      console.error(e);
    }
    onDone();
  }

  return (
    <div className="fixed inset-0 z-[100] flex items-center justify-center bg-gradient-to-br from-background via-background to-primary/10 text-foreground">
      <div className="flex w-full max-w-xl flex-col items-center gap-6 px-6 py-10">
        {/* Progress dots */}
        <div className="flex items-center gap-2">
          {ORDER.map((s, i) => (
            <span
              key={s}
              className={cn(
                "h-1.5 w-6 rounded-full transition-colors",
                i <= currentIndex ? "bg-primary" : "bg-muted",
              )}
            />
          ))}
        </div>

        {step === "welcome" && <WelcomeStep />}
        {step === "microphone" && (
          <PermissionStep
            icon={Mic}
            title={t("onboarding.step.microphoneTitle")}
            description={t("onboarding.step.microphoneDescription")}
            ok={perms?.microphone.ok ?? false}
            statusLabel={
              perms?.microphone.label_key
                ? t(perms.microphone.label_key, perms.microphone.label_args ?? undefined)
                : undefined
            }
            primaryAction={{
              label: t("onboarding.step.microphoneAction"),
              onClick: () => api.openPrivacyMicrophone(),
            }}
          />
        )}
        {step === "ocr" && (
          <PermissionStep
            icon={Languages}
            title={t("onboarding.step.ocrTitle")}
            description={t("onboarding.step.ocrDescription")}
            ok={perms?.ocr.ok ?? false}
            statusLabel={
              perms?.ocr.label_key
                ? t(perms.ocr.label_key, perms.ocr.label_args ?? undefined)
                : undefined
            }
            primaryAction={{
              label: t("onboarding.step.ocrAction"),
              onClick: () => api.openLanguageSettings(),
            }}
          />
        )}
        {step === "autostart" && (
          <PermissionStep
            icon={Power}
            title={t("onboarding.step.autostartTitle")}
            description={t("onboarding.step.autostartDescription")}
            ok={perms?.autostart.ok ?? false}
            statusLabel={
              perms?.autostart.label_key
                ? t(perms.autostart.label_key, perms.autostart.label_args ?? undefined)
                : undefined
            }
            primaryAction={
              perms?.autostart
                ? {
                    label: perms.autostart.ok
                      ? t("permissions.autostartDeactivate")
                      : t("permissions.autostartActivate"),
                    onClick: async () => {
                      await api.setAutostartEnabled(!perms.autostart.ok);
                      await refresh();
                    },
                  }
                : undefined
            }
          />
        )}
        {step === "hotkey" && (
          <PermissionStep
            icon={Keyboard}
            title={t("onboarding.step.hotkeyTitle")}
            description={t("onboarding.step.hotkeyDescription")}
            ok
            statusLabel={t("onboarding.step.hotkeyStatus")}
          />
        )}

        <div className="mt-4 flex w-full items-center justify-between">
          <div>
            {currentIndex > 0 && (
              <Button variant="ghost" size="sm" onClick={back}>
                {t("common.back")}
              </Button>
            )}
          </div>
          <div className="flex items-center gap-2">
            <Button variant="ghost" size="sm" onClick={finish}>
              {step === "welcome"
                ? t("onboarding.skipEarly")
                : t("onboarding.skipLate")}
            </Button>
            <Button size="sm" onClick={next}>
              {step === "welcome"
                ? t("onboarding.ready.start")
                : currentIndex === ORDER.length - 1
                  ? t("onboarding.finish")
                  : t("onboarding.continue")}
              <ArrowRight className="h-3.5 w-3.5" />
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}

function WelcomeStep() {
  const { t } = useTranslation();
  return (
    <div className="flex flex-col items-center gap-4 text-center">
      <img
        src="/favicon.png"
        alt="Parla"
        className="h-20 w-20 rounded-2xl shadow-md"
      />
      <h1 className="text-3xl font-black leading-tight">
        {t("onboarding.welcome.title")}
      </h1>
      <p className="max-w-prose text-sm text-muted-foreground">
        {t("onboarding.welcome.description")}
      </p>
    </div>
  );
}

function PermissionStep({
  icon: Icon,
  title,
  description,
  ok,
  statusLabel,
  primaryAction,
}: {
  icon: LucideIcon;
  title: string;
  description: string;
  ok: boolean;
  statusLabel?: string;
  primaryAction?: { label: string; onClick: () => void };
}) {
  return (
    <div className="flex w-full flex-col items-center gap-3 rounded-xl border bg-card p-8 text-center">
      <div
        className={cn(
          "flex h-14 w-14 items-center justify-center rounded-full",
          ok
            ? "bg-green-500/10 text-green-600 dark:text-green-400"
            : "bg-amber-500/10 text-amber-600 dark:text-amber-400",
        )}
      >
        {ok ? <Check className="h-7 w-7" /> : <Icon className="h-7 w-7" />}
      </div>
      <h2 className="text-xl font-bold">{title}</h2>
      <p className="max-w-prose text-sm text-muted-foreground">
        {description}
      </p>
      {statusLabel && (
        <p
          className={cn(
            "mt-1 text-xs font-medium",
            ok ? "text-green-600 dark:text-green-400" : "text-amber-600 dark:text-amber-400",
          )}
        >
          {statusLabel}
        </p>
      )}
      {primaryAction && (
        <Button size="sm" variant="outline" onClick={primaryAction.onClick}>
          {primaryAction.label}
        </Button>
      )}
    </div>
  );
}
