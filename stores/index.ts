import { defineStore } from 'pinia'

export const useIndex = defineStore('index', {
    state: () => ({
        darkMode: false,
        showAlert: false,
        alertVariant: 'success',
        alertMsg: '',
        sseConnected: false,
    }),

    getters: {},
    actions: {
        msgAlert(variance: string, text: string, seconds: number = 3) {
            this.alertVariant = variance
            this.alertMsg = text
            this.showAlert = true

            setTimeout(() => {
                this.showAlert = false
                this.alertVariant = 'success'
                this.alertMsg = ''
            }, seconds * 1000)
        },
    },
})
