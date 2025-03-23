## Add Language

You are very welcome to add more languages! Just copy en-US.js to the correct target Country code and modify the content.

When you are done with the translation add the filename to [nuxt.config.ts](../nuxt.config.ts), in section:

```DIFF
i18n: {
    locales: [
        {
            code: 'de',
            name: 'Deutsch',
            file: 'de-DE.js',
        },
        {
            code: 'en',
            name: 'English',
            file: 'en-US.js',
        },
+        {
+            code: '<SHORT CODE>',
+            name: '<NAME>',
+            file: '<CODE>.js',
+        },
    ],

    ...
```

And also add the new link paths:

```DIFF
i18n: {
    locales: [...],
    ...
    pages: {
        'player': {
            de: '/wiedergabe',
            en: '/player',
+           <CODE>: /<PATH>,
        },

    ...
```
