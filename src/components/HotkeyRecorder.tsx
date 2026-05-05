// Modal qui capture une combinaison de touches libre pour creer un
// trigger Custom (ex Ctrl+Alt+R, Win+Shift+Space, F13).
//
// Implementation : un dialog plein ecran avec un keydown listener au
// niveau document. On capture la premiere combinaison qui inclut au
// moins un modifier ET une touche finale non-modifier. ESC annule.
//
// Reference VoiceInk : KeyboardShortcuts.Recorder fait exactement la
// meme chose dans Settings. On ne capture pas les touches sticky
// (CapsLock, NumLock) ni les media keys.

import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { X } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { HotkeyTrigger } from "@/lib/tauri";

const MODIFIER_VKS = new Set([
  0xa0, 0xa1, // Shift L/R
  0xa2, 0xa3, // Ctrl L/R
  0xa4, 0xa5, // Alt L/R
  0x5b, 0x5c, // Win L/R
  0x14, // CapsLock
  0x90, 0x91, // NumLock, ScrollLock
]);

type Captured = {
  vk: number;
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  win: boolean;
  label: string;
};

export function HotkeyRecorder({
  open,
  onCancel,
  onCapture,
}: {
  open: boolean;
  onCancel: () => void;
  onCapture: (trigger: Extract<HotkeyTrigger, { kind: "combo" }>) => void;
}) {
  const { t } = useTranslation();
  const [captured, setCaptured] = useState<Captured | null>(null);
  const finishedRef = useRef(false);

  // Reset l'etat a chaque ouverture.
  useEffect(() => {
    if (open) {
      setCaptured(null);
      finishedRef.current = false;
    }
  }, [open]);

  useEffect(() => {
    if (!open) return;

    function onKey(e: KeyboardEvent) {
      e.preventDefault();
      e.stopPropagation();

      if (e.key === "Escape" && !e.ctrlKey && !e.altKey && !e.shiftKey) {
        onCancel();
        return;
      }

      // VK code lu via DOM event keyCode (deprecated mais fiable pour
      // les touches non-imprimables sur Windows). Pour les touches
      // alphanumeriques, e.key.toUpperCase().charCodeAt(0) suffit.
      const vk = e.keyCode || (e.key.length === 1 ? e.key.toUpperCase().charCodeAt(0) : 0);
      if (!vk) return;

      // Si le user appuie uniquement sur un modifier, on ne valide pas.
      // Il faut une touche finale (lettre, chiffre, F-key, espace, etc.).
      if (MODIFIER_VKS.has(vk)) return;

      if (finishedRef.current) return;
      finishedRef.current = true;

      const trigger: Extract<HotkeyTrigger, { kind: "combo" }> = {
        kind: "combo",
        vk,
        ctrl: e.ctrlKey,
        alt: e.altKey,
        shift: e.shiftKey,
        win: e.metaKey,
      };

      const label = formatCombo(trigger);
      setCaptured({ ...trigger, label });

      // On laisse 600ms a l'utilisateur pour voir ce qu'il a capture
      // avant de fermer + commit.
      window.setTimeout(() => onCapture(trigger), 600);
    }

    document.addEventListener("keydown", onKey, { capture: true });
    return () => {
      document.removeEventListener("keydown", onKey, { capture: true });
    };
  }, [open, onCancel, onCapture]);

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm"
      onClick={onCancel}
    >
      <div
        className="relative w-full max-w-sm rounded-lg border bg-background p-6 shadow-lg"
        onClick={(e) => e.stopPropagation()}
      >
        <button
          type="button"
          onClick={onCancel}
          className="absolute right-3 top-3 text-muted-foreground hover:text-foreground"
          aria-label={t("common.close")}
        >
          <X className="h-4 w-4" />
        </button>

        <h2 className="text-base font-semibold">
          {t("hotkey.record.title")}
        </h2>
        <p className="mt-1 text-xs text-muted-foreground">
          {t("hotkey.record.hint")}
        </p>

        <div className="mt-6 flex h-16 items-center justify-center rounded-md border-2 border-dashed bg-muted/30">
          {captured ? (
            <span className="font-mono text-base">{captured.label}</span>
          ) : (
            <span className="text-sm text-muted-foreground">
              {t("hotkey.record.waiting")}
            </span>
          )}
        </div>

        <div className="mt-4 flex justify-end gap-2">
          <Button variant="ghost" size="sm" onClick={onCancel}>
            {t("common.cancel")}
          </Button>
        </div>
      </div>
    </div>
  );
}

const VK_NAMES: Record<number, string> = {
  0x08: "Backspace",
  0x09: "Tab",
  0x0d: "Enter",
  0x10: "Shift",
  0x11: "Ctrl",
  0x12: "Alt",
  0x14: "CapsLock",
  0x1b: "Esc",
  0x20: "Space",
  0x21: "PageUp",
  0x22: "PageDown",
  0x23: "End",
  0x24: "Home",
  0x25: "Left",
  0x26: "Up",
  0x27: "Right",
  0x28: "Down",
  0x2c: "PrintScreen",
  0x2d: "Insert",
  0x2e: "Delete",
  0x70: "F1",
  0x71: "F2",
  0x72: "F3",
  0x73: "F4",
  0x74: "F5",
  0x75: "F6",
  0x76: "F7",
  0x77: "F8",
  0x78: "F9",
  0x79: "F10",
  0x7a: "F11",
  0x7b: "F12",
  0x7c: "F13",
  0x7d: "F14",
  0x7e: "F15",
  0x7f: "F16",
};

export function vkLabel(vk: number): string {
  if (VK_NAMES[vk]) return VK_NAMES[vk];
  if (vk >= 0x30 && vk <= 0x39) return String.fromCharCode(vk); // 0..9
  if (vk >= 0x41 && vk <= 0x5a) return String.fromCharCode(vk); // A..Z
  return `VK_${vk.toString(16).toUpperCase().padStart(2, "0")}`;
}

export function formatCombo(
  combo: Extract<HotkeyTrigger, { kind: "combo" }>,
): string {
  const parts: string[] = [];
  if (combo.ctrl) parts.push("Ctrl");
  if (combo.alt) parts.push("Alt");
  if (combo.shift) parts.push("Shift");
  if (combo.win) parts.push("Win");
  parts.push(vkLabel(combo.vk));
  return parts.join(" + ");
}
