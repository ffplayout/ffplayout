import { createI18n } from 'vue-i18n'

import deDE from './locales/de-DE.ts'
import enUS from './locales/en-US.ts'
import ptBR from './locales/pt-BR.ts'
import ruRU from './locales/ru-RU.ts'

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
]

const instance = createI18n({
    legacy: false,
    locale: 'en-US',
    messages: {
        'de': deDE,
        'en': enUS,
        'pt-br': ptBR,
        'ru': ruRU,
    },
})

export default instance

export const i18n = instance.global
