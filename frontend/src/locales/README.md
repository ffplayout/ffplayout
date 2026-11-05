## Translations

The frontend loads translations from `de-DE.ts`, `en-US.ts`, `pt-BR.ts` and
`ru-RU.ts`. English is the fallback language. Language registration and the
language selector are defined in [i18n.ts](../i18n.ts).

To add a language, copy `en-US.ts`, translate the values and register the new
locale in `i18n.ts`. Keep all keys and named placeholders such as `{id}` and
`{value}` consistent across languages. Preserve protocol names, FFmpeg options,
filter labels and example commands.

Check labels in their actual UI context: opacity is the opposite of transparency,
In/Out refer to clip cut points, and Previous refers to the preceding clip.
Help text should use the localized button labels and describe current behavior.

Run the translation and graphics form checks from the repository root:

```sh
npm run test:graphics
npm run type-check
```
