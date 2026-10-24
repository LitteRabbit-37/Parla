// i18n setup - react-i18next with browser language detection.
// Supported languages: English (fallback), French, Spanish, German.
//
// The user language is detected in this order: manual override in localStorage,
// browser navigator, app default (en). It's persisted in localStorage under
// the key `parla_language`.

import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import LanguageDetector from "i18next-browser-languagedetector";

import en from "./locales/en.json";
import fr from "./locales/fr.json";
import es from "./locales/es.json";
import de from "./locales/de.json";

export const SUPPORTED_LANGUAGES = ["en", "fr", "es", "de"] as const;
export type SupportedLanguage = (typeof SUPPORTED_LANGUAGES)[number];

export const LANGUAGE_LABELS: Record<SupportedLanguage, string> = {
  en: "English",
  fr: "Francais",
  es: "Espanol",
  de: "Deutsch",
};

i18n
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources: {
      en: { translation: en },
      fr: { translation: fr },
      es: { translation: es },
      de: { translation: de },
    },
    fallbackLng: "en",
    supportedLngs: SUPPORTED_LANGUAGES,
    interpolation: {
      escapeValue: false,
    },
    detection: {
      order: ["localStorage", "navigator"],
      lookupLocalStorage: "parla_language",
      caches: ["localStorage"],
    },
  });

export default i18n;
