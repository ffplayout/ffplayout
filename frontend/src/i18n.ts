import { createI18n } from 'vue-i18n'

import deDE from './locales/de-DE.ts'
import enUS from './locales/en-US.ts'
import ptBR from './locales/pt-BR.ts'
import ruRU from './locales/ru-RU.ts'
import zhCN from './locales/zh-CN.ts'

export const locales = [
    {
        code: 'de',
        language: 'de-DE',
        name: 'Deutsch',
    },
    {
        code: 'en',
        language: 'en-US',
        name: 'English',
    },
    {
        code: 'pt-br',
        language: 'pt-BR',
        name: 'Português (BR)',
    },
    {
        code: 'ru',
        language: 'ru-RU',
        name: 'Русский язык (RU)',
    },
    {
        code: 'zh',
        language: 'zh-CN',
        name: '简体中文',
    },
]

const defaultLocale = 'en'

function initialLocale(): string {
    try {
        const saved = localStorage.getItem('language')
        const selected = locales.find(({ code }) => code === saved)
        if (selected) return selected.code
    } catch {
        // Browser language detection still works when storage is blocked.
    }

    for (const preferred of globalThis.navigator?.languages ?? []) {
        const tag = preferred.toLowerCase()
        const language = tag.split('-')[0]
        const supported =
            locales.find((locale) => locale.language.toLowerCase() === tag || locale.code === tag) ??
            locales.find((locale) => locale.language.toLowerCase().split('-')[0] === language)
        if (supported) return supported.code
    }

    return defaultLocale
}

const instance = createI18n({
    legacy: false,
    locale: initialLocale(),
    fallbackLocale: defaultLocale,
    messages: {
        de: deDE,
        en: enUS,
        'pt-br': ptBR,
        ru: ruRU,
        zh: zhCN,
    },
})

export default instance

export const i18n = instance.global
