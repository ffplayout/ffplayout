import { createI18n } from 'vue-i18n'

import deDE from './de-DE.ts'
import enUS from './en-US.ts'
import ptBR from './pt-BR.ts'
import ruRU from './ru-RU.ts'

const instance = createI18n({
    legacy: false,
    locale: 'en-US',
    messages: {
        'de-DE': deDE,
        'en-US': enUS,
        'pt-BR': ptBR,
        'ru-RU': ruRU,
    },
})

export default instance

export const i18n = instance.global
