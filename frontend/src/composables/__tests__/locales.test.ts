import assert from 'node:assert/strict'
import { test } from 'node:test'
import { createI18n } from 'vue-i18n'
import de from '../../locales/de-DE.ts'
import en from '../../locales/en-US.ts'
import pt from '../../locales/pt-BR.ts'
import ru from '../../locales/ru-RU.ts'

function entries(messages: object, prefix = ''): [string, string][] {
    return Object.entries(messages).flatMap(([key, value]) =>
        typeof value === 'object' ? entries(value, `${prefix}${key}.`) : [[`${prefix}${key}`, value]],
    )
}

for (const [locale, messages] of Object.entries({ de, en, 'pt-br': pt, ru })) {
    test(`${locale} supplies all translation keys and valid message syntax`, () => {
        const expected = entries(en)
            .map(([key]) => key)
            .sort()
        const actual = entries(messages)
        assert.deepEqual(actual.map(([key]) => key).sort(), expected)
        const i18n = createI18n({
            legacy: false,
            locale,
            messages: { [locale]: messages },
            warnHtmlMessage: false,
        })
        for (const [key, value] of actual) {
            assert.ok(value.trim(), `${key} must not be empty`)
            // Message compilation throws on malformed interpolation or linked messages.
            assert.doesNotThrow(() => i18n.global.t(key), key)
        }
    })
}
