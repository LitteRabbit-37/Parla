const assert = require("node:assert/strict");
const { test } = require("node:test");
const fs = require("node:fs");
const path = require("node:path");
const { createRequire } = require("node:module");
const ts = require("typescript");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { createInstance } = require("i18next");
const { I18nextProvider, initReactI18next } = require("react-i18next");

const root = path.resolve(__dirname, "..");
const read = (file) => fs.readFileSync(path.join(root, file), "utf8");
const languages = ["en", "fr", "es", "de"];
const resources = Object.fromEntries(languages.map((lang) => [
  lang, { translation: JSON.parse(read(`src/i18n/locales/${lang}.json`)) },
]));

// Use the project's TypeScript compiler to load real components and helpers.
// No separate test framework or copy of the production rendering logic.
const modules = new Map();
function loadSource(file) {
  const filename = path.resolve(root, file);
  if (modules.has(filename)) return modules.get(filename).exports;
  const module = { exports: {} };
  modules.set(filename, module);
  const nativeRequire = createRequire(filename);
  const requireSource = (specifier) => {
    if (!specifier.startsWith(".") && !specifier.startsWith("@/")) {
      return nativeRequire(specifier);
    }
    const base = specifier.startsWith("@/")
      ? path.join(root, "src", specifier.slice(2))
      : path.resolve(path.dirname(filename), specifier);
    const source = [base, `${base}.ts`, `${base}.tsx`, `${base}/index.ts`]
      .find((candidate) => /\.tsx?$/.test(candidate) && fs.existsSync(candidate));
    return source ? loadSource(source) : nativeRequire(base);
  };
  const { outputText } = ts.transpileModule(fs.readFileSync(filename, "utf8"), {
    compilerOptions: {
      target: ts.ScriptTarget.ES2020,
      module: ts.ModuleKind.CommonJS,
      jsx: ts.JsxEmit.ReactJSX,
      esModuleInterop: true,
    },
  });
  new Function("require", "module", "exports", outputText)(requireSource, module, module.exports);
  return module.exports;
}

const { localModelText, cloudModelText, formatModelBytes } = loadSource("src/lib/modelLabels.ts");
const { languageLabel } = loadSource("src/lib/languages.ts");
const { resolveDefaultDisplayName } = loadSource("src/components/models/types.ts");
const { WhisperModelCard } = loadSource("src/components/models/WhisperModelCard.tsx");
const { ParakeetModelCard } = loadSource("src/components/models/ParakeetModelCard.tsx");
const { CloudModelCard } = loadSource("src/components/models/CloudModelCard.tsx");

// Read the real Rust catalogs: adding a model without translations must fail.
function catalog(file, struct) {
  const pattern = new RegExp(`^    ${struct} \\{([\\s\\S]*?)\\n    \\}`, "gm");
  const models = [...read(file).matchAll(pattern)].map((match) => {
    const field = (name) => match[1].match(new RegExp(`\\b${name}: "([^"]*)"`))?.[1];
    return {
      id: field("id"), provider_id: field("provider_id"), model_id: field("model_id"),
      display_name: field("display_name"), notes: field("notes"),
      imported: false, downloaded: false, size_bytes: 2500000000,
      on_disk_bytes: null, missing_files: [], multilingual: false, is_quantized: false,
      speed: 0.9, accuracy: 0.9,
    };
  });
  assert.ok(models.length > 0, `Failed to read catalog: ${file}`);
  for (const model of models) assert.ok(model.display_name && model.notes);
  return models;
}
const catalogs = {
  whisper: catalog("src-tauri/src/transcription/model.rs", "WhisperModelInfo"),
  parakeet: catalog("src-tauri/src/transcription/parakeet_model_manager.rs", "ParakeetVariant"),
  cloud: catalog("src-tauri/src/transcription/cloud/catalog.rs", "CloudModelInfo"),
  llm: catalog("src-tauri/src/enhancement/model_manager.rs", "GgufModelInfo"),
};
const flatten = (value, prefix = "") => Object.entries(value).flatMap(([key, item]) =>
  typeof item === "object" ? flatten(item, `${prefix}${key}.`) : [[prefix + key, item]],
);
const escaped = (text) => renderToStaticMarkup(React.createElement("span", null, text)).slice(6, -7);
const render = (component, props, i18n) => renderToStaticMarkup(
  React.createElement(I18nextProvider, { i18n }, React.createElement(component, props)),
);
async function translator(lng) {
  const i18n = createInstance();
  await i18n.use(initReactI18next).init({
    lng, fallbackLng: "en", resources, interpolation: { escapeValue: false },
  });
  return i18n;
}

test("all catalog keys and interpolation placeholders exist in every locale", () => {
  const english = new Map(flatten(resources.en.translation.modelCatalog));
  for (const lang of languages) {
    const actual = new Map(flatten(resources[lang].translation.modelCatalog));
    assert.deepEqual([...actual.keys()], [...english.keys()], lang);
    for (const [key, value] of actual) {
      assert.equal(typeof value, "string");
      assert.ok(value.trim(), `${lang}: ${key}`);
      assert.deepEqual(value.match(/\{\{.*?\}\}/g), english.get(key).match(/\{\{.*?\}\}/g));
    }
  }
});

for (const lang of languages) {
  test(`${lang}: every backend model uses translated catalog text in actual cards`, async () => {
    const i18n = await translator(lang);
    const usedKeys = new Set();
    const t = (key, options) => {
      assert.equal(i18n.exists(key, { lng: lang, fallbackLng: false }), true, `${lang}: ${key}`);
      usedKeys.add(key);
      return i18n.t(key, options);
    };
    for (const [kind, models] of Object.entries(catalogs)) {
      for (const model of models) {
        const before = usedKeys.size;
        const text = kind === "cloud" ? cloudModelText(t, model) : localModelText(t, kind, model);
        assert.ok(usedKeys.size > before, `Untranslated catalog entry: ${kind}/${model.id ?? model.model_id}`);
        const component = { whisper: WhisperModelCard, parakeet: ParakeetModelCard, cloud: CloudModelCard }[kind];
        if (component) {
          const html = render(component, {
            model, isCurrent: false, isConfigured: false, providerName: model.provider_id,
            progress: null, status: null, error: null,
          }, i18n);
          assert.ok(html.includes(escaped(text.name)), `${kind}: rendered name`);
          assert.ok(html.includes(escaped(text.notes)), `${kind}: rendered notes`);
        }
        if (kind === "cloud" || kind === "llm") assert.equal(text.name, model.display_name);
      }
    }
  });
}

test("the reported Parakeet text and default-model heading follow language changes", async () => {
  const i18n = await translator("de");
  const model = catalogs.parakeet[0];
  const source = { kind: "parakeet", parakeet_model_id: model.id };
  const expected = {
    de: ["Englisch", "Referenzmodell. Nur Englisch."],
    en: ["English", "Reference model. English only."],
    fr: ["anglais", "Modèle de référence. Anglais uniquement."],
    es: ["inglés", "Modelo de referencia. Solo inglés."],
  };
  for (const lang of ["de", "en", "fr", "es", "de"]) {
    await i18n.changeLanguage(lang);
    const text = localModelText(i18n.t, "parakeet", model);
    assert.equal(text.name, `Parakeet TDT 0.6B v2 (${expected[lang][0]}, F16)`);
    assert.ok(text.notes.startsWith(expected[lang][1]));
    assert.equal(resolveDefaultDisplayName(i18n.t, source, [], [model], []), text.name);
  }
});

test("imported names and unknown models survive; cloud identity includes the provider", async () => {
  const i18n = await translator("de");
  for (const kind of ["whisper", "llm"]) {
    const model = { ...catalogs[kind][0], imported: true, display_name: "Mein {{name}} <Modell>" };
    const text = localModelText(i18n.t, kind, model);
    assert.equal(text.name, model.display_name);
    assert.match(text.notes, /^Vom Nutzer importiertes/);
  }
  for (const id of ["new-model", "constructor", "__proto__"]) {
    const model = { id, display_name: "Unknown {{model}}", notes: "Original {{notes}}" };
    assert.deepEqual(localModelText(i18n.t, "whisper", model), { name: model.display_name, notes: model.notes });
  }
  const wrongProvider = { ...catalogs.cloud[0], provider_id: "custom" };
  assert.deepEqual(cloudModelText(i18n.t, wrongProvider), { name: wrongProvider.display_name, notes: wrongProvider.notes });
  assert.equal(resolveDefaultDisplayName(i18n.t, null, [], [], []), null);
  assert.equal(resolveDefaultDisplayName(i18n.t, { kind: "local", whisper_model_id: "missing" }, [], [], []), null);
});

test("model text falls back to English when a translation is unavailable", async () => {
  const i18n = await translator("it");
  assert.match(localModelText(i18n.t, "parakeet", catalogs.parakeet[0]).notes, /^Reference model/);
});

test("language names and model sizes use the selected locale without changing codes", () => {
  const expected = { en: "German", fr: "allemand", es: "alemán", de: "Deutsch" };
  for (const lang of languages) {
    const names = new Intl.DisplayNames([lang], { type: "language", fallback: "none" });
    assert.equal(languageLabel("de", names), expected[lang]);
    assert.ok(languageLabel("en_uk", names));
    assert.equal(languageLabel("invalid_code!", names), "INVALID_CODE!");
  }
  assert.match(formatModelBytes(2500000000, "de"), /2,5\s*GB/);
  assert.match(formatModelBytes(2500000000, "fr"), /2,5\s*Go/);
  assert.match(formatModelBytes(2500000000, "en"), /2\.5\s*GB/);
  assert.equal(formatModelBytes(null, "de"), "-");
});
