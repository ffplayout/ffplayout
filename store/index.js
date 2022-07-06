export const strict = false

export const state = () => ({
    showErrorAlert: false,
    variant: 'danger',
    ErrorAlertMessage: ''
})

export const mutations = {
    UPDATE_SHOW_ERROR_ALERT (state, show) {
        state.showErrorAlert = show
    },
    UPDATE_VARIANT (state, variant) {
        state.variant = variant
    },
    UPDATE_ERROR_ALERT_MESSAGE (state, message) {
        state.ErrorAlertMessage = message
    }
}
