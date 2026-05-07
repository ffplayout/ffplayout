import { defineStore } from 'pinia'

export const useIndex = defineStore('index', {
    state: () => ({
        darkMode: false,
        showAlert: false,
        alertVariant: 'success',
        alertMsg: '',
        sseConnected: false,
        severityLevels: {
            DEBUG: 1,
            INFO: 2,
            WARN: 3,
            ERROR: 4,
        } as { [key: string]: number },
    }),

    getters: {},
    actions: {
        msgAlert(variance: string, text: string, seconds: number = 3) {
            this.alertVariant = variance
            this.alertMsg = this.stringifyAlertText(text)
            this.showAlert = true

            setTimeout(() => {
                this.showAlert = false
                this.alertVariant = 'success'
                this.alertMsg = ''
            }, seconds * 1000)
        },

        stringifyAlertText(text: unknown): string {
            if (typeof text === 'string') {
                return text
            }

            if (text instanceof Error) {
                return text.message
            }

            if (text && typeof text === 'object') {
                const maybeError = (text as { error?: unknown }).error

                if (typeof maybeError !== 'undefined') {
                    if (typeof maybeError === 'string') {
                        return maybeError
                    }

                    if (maybeError instanceof Error) {
                        return maybeError.message
                    }

                    try {
                        return JSON.stringify(maybeError)
                    }
                    catch {
                        return String(maybeError)
                    }
                }

                try {
                    return JSON.stringify(text)
                }
                catch {
                    return String(text)
                }
            }

            return String(text)
        },
    },
})
