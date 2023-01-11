import { defineStore } from 'pinia'

export const useIndex = defineStore('index', {
    state: () => ({
        showAlert: false,
        alertVariant: 'success',
        alertMsg: '',
    }),

    getters: {},
    actions: {
        resetAlert() {
            this.showAlert = false
            this.alertVariant = 'success'
            this.alertMsg = ''
        },
    },
})
