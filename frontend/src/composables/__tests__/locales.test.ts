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

function placeholders(value: string): string[] {
    return [...new Set([...value.matchAll(/\{\s*([a-zA-Z_]\w*|\d+)\s*\}/g)].map((match) => match[1]))].sort()
}

for (const [locale, messages] of Object.entries({ de, en, 'pt-br': pt, ru })) {
    test(`${locale} supplies all translation keys and valid message syntax`, () => {
        const expected = entries(en)
            .map(([key]) => key)
            .sort()
        const actual = entries(messages)
        const reference = new Map(entries(en))
        assert.deepEqual(actual.map(([key]) => key).sort(), expected)
        const i18n = createI18n({
            legacy: false,
            locale,
            messages: { [locale]: messages },
            warnHtmlMessage: false,
        })
        for (const [key, value] of actual) {
            assert.ok(value.trim(), `${key} must not be empty`)
            assert.deepEqual(placeholders(value), placeholders(reference.get(key)!), `${key} placeholders`)
            // Message compilation throws on malformed interpolation or linked messages.
            assert.doesNotThrow(() => i18n.global.t(key), key)
        }
    })
}
