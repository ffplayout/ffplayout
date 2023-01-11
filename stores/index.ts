import { defineStore } from 'pinia'

export const useIndex = defineStore('index', {
    state: () => ({
        showAlert: false,
        alertVariant: 'alert-success',
        alertMsg: '',
    }),

    getters: {},
    actions: {
        resetAlert() {
            this.showAlert = false
            this.alertVariant = 'alert-success'
            this.alertMsg = ''
        },
    },
})
