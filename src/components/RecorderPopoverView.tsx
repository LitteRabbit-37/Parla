// Popover du bouton Mode de la bulle, rendu dans sa propre fenetre
// ("recorder-popover", voir src-tauri/src/mini_recorder.rs).
//
// Reference VoiceInk Features/Modes/Views/ModePopover.swift : titre
// "Select Mode", liste des modes actives (ModeRow : icone, nom, coche sur
// le mode effectif), clic = setActiveConfiguration. Vide : "No Modes
// Available". 180 pt de large, 340 max de haut.
//
// La fenetre etant separee de la bulle, le survol est remonte a la bulle
// par l'event "recorder:popover-hover" pour reproduire la logique
// RecorderModeButton.syncPopoverVisibility (fermeture 250 ms apres avoir
// quitte le bouton ET le popover).

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { emit, listen } from "@tauri-apps/api/event";
import { Check, Settings as SettingsIcon } from "lucide-react";
import { api, type PowerModeConfig, type PowerSession } from "@/lib/tauri";
import { cn, powerShortcutLabel } from "@/lib/utils";

export function RecorderPopoverView() {
  const { t } = useTranslation();
  const [configs, setConfigs] = useState<PowerModeConfig[]>([]);
  const [session, setSession] = useState<PowerSession | null>(null);

  useEffect(() => {
    const load = () => {
      Promise.all([api.listPowerConfigs(), api.getActivePowerSession()])
        .then(([cfgs, s]) => {
          setConfigs(cfgs);
          setSession(s);
        })
        .catch(console.error);
    };
    load();
    const unlistens = [
      listen<PowerSession | null>("power_mode:active", (e) => setSession(e.payload)),
      // La bulle demande un rechargement a chaque ouverture (profils
      // eventuellement modifies depuis la creation de la fenetre).
      listen("recorder:popover-refresh", load),
    ];
    return () => {
      Promise.all(unlistens).then((arr) => arr.forEach((fn) => fn()));
    };
  }, []);

  const hover = (hovering: boolean) => {
    emit("recorder:popover-hover", { hovering }).catch(console.error);
  };

  async function select(c: PowerModeConfig) {
    try {
      await api.selectPowerConfig(c.id);
    } catch (e) {
      console.error(e);
    }
    hover(false);
    api.closeRecorderPopover().catch(console.error);
  }

  const enabled = configs.filter((c) => c.is_enabled);

  return (
    <div
      className="flex h-screen w-screen items-end bg-transparent p-1"
      onMouseEnter={() => hover(true)}
      onMouseLeave={() => hover(false)}
    >
      <div className="flex max-h-full w-full flex-col overflow-hidden rounded-lg border bg-popover text-popover-foreground shadow-lg">
        <p className="px-3 pb-1.5 pt-2.5 text-xs font-semibold">
          {t("miniRecorder.selectMode")}
        </p>
        <div className="h-px bg-border" />
        <div className="overflow-auto p-1">
          {enabled.length === 0 ? (
            <div className="px-2 py-2 text-xs">
              <p className="mb-1 font-medium">{t("miniRecorder.noModesAvailable")}</p>
              <p className="text-muted-foreground">{t("miniRecorder.noConfigs")}</p>
              <button
                type="button"
                onClick={async () => {
                  try {
                    await api.showMainWindow("powermode");
                  } catch (e) {
                    console.error(e);
                  }
                  hover(false);
                  api.closeRecorderPopover().catch(console.error);
                }}
                className="mt-2 inline-flex items-center gap-1.5 rounded-md bg-accent px-2 py-1 text-[11px] font-medium hover:bg-accent/80"
              >
                <SettingsIcon className="h-3 w-3" />
                {t("miniRecorder.openPowerMode")}
              </button>
            </div>
          ) : (
            <>
              {enabled.map((c, idx) => {
                // Meme mapping que le backend : le raccourci cible le Nieme
                // profil ACTIVE dans l'ordre stocke.
                const shortcut = powerShortcutLabel(idx);
                const isActive = c.id === session?.config_id;
                return (
                  <button
                    key={c.id}
                    type="button"
                    onClick={() => select(c)}
                    className={cn(
                      "flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-xs hover:bg-accent",
                      isActive && "bg-accent",
                    )}
                  >
                    <span className="text-lg leading-none">{c.emoji}</span>
                    <span className="flex-1 truncate">{c.name}</span>
                    {shortcut && (
                      <kbd className="rounded border bg-muted px-1 py-0.5 font-mono text-[9px] font-medium text-muted-foreground">
                        {shortcut}
                      </kbd>
                    )}
                    {isActive && <Check className="h-3 w-3" />}
                  </button>
                );
              })}
              <p className="mt-1 px-2 pb-1 text-[10px] text-muted-foreground">
                {t("miniRecorder.autoSwitch")}
              </p>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
