# Contributing to Parla

Thanks for your interest in Parla. Contributions of all sizes are welcome, from bug reports to feature suggestions to code.

## Reporting bugs

- Search the [existing issues](https://github.com/LitteRabbit-37/Parla/issues) first to avoid duplicates.
- Include your Windows version, whether you have an NVIDIA GPU (and driver version), the Parla version, and as much reproduction detail as possible.
- If possible, attach the Parla log excerpt (the app writes to `%LOCALAPPDATA%\Parla\logs` by default).

## Suggesting features

Open a discussion or issue with the `enhancement` label. Please describe the problem you are trying to solve first, then the proposed solution. Behavioral references to the [VoiceInk](https://github.com/Beingpax/VoiceInk) macOS app are welcome - Parla aims for feature parity.

## Pull requests

1. Fork the repo and create a branch from `main`.
2. Make your changes, keeping commits focused and well-described.
3. Run the Rust test suite: `cd src-tauri && cargo test --lib`.
4. Run the frontend type-check: `npx tsc --noEmit`.
5. Update the relevant documentation (README, BUILDING, inline comments) if your change affects them.
6. Open the PR with a clear title and description.

## Translations

Parla ships with French, English, Spanish and German translations under `src/i18n/locales/`. If you want to add another language or improve an existing translation:

- Copy `en.json` to `<lang>.json`
- Translate the values (keys stay untouched)
- Import and register the new language in `src/i18n/index.ts` (`resources`, `SUPPORTED_LANGUAGES` and `LANGUAGE_LABELS`)
- Add native tray translations in `src-tauri/src/tray.rs`
- Run `npm run test:i18n` and `npx tsc --noEmit`

Built-in model names and descriptions are translated through `modelCatalog` in each locale. When adding a model to a Rust catalog, also add its stable ID mapping in `src/lib/modelLabels.ts` and the corresponding keys in every locale. Cloud mappings include the provider ID. Keep product names and user-chosen names intact. The localization tests check catalog coverage against the Rust sources and render model cards in every supported language.

Translation PRs are especially welcome and easy to review.

## Code style

- Rust: `cargo fmt` + `cargo clippy --lib` before committing. Keep module headers documenting the VoiceInk reference file when applicable.
- TypeScript: default Vite / TSX conventions. Tailwind classes grouped by concern (layout, spacing, colors).
- No emojis in code, commit messages or public documentation.
- Git commit messages in the imperative mood, English.

## License

By contributing, you agree that your contributions will be licensed under the GPL-3.0 license that covers the project.
