import { defineStore } from 'pinia'

export const useIndex = defineStore('index', {
    state: () => ({
        showAlert: false,
        alertVariant: 'alert-success',
        alertMsg: '',
    }),

    getters: {},
    actions: {
        msgAlert(variance: string, text: string, seconds: number = 3) {
            this.alertVariant = variance
            this.alertMsg = text
            this.showAlert = true

            setTimeout(() => {
                this.showAlert = false
                this.alertVariant = 'alert-success'
                this.alertMsg = ''
            }, seconds * 1000);
        },
    },
})
