export const strict = false

export const state = () => ({
    showErrorAlert: false,
    ErrorAlertMessage: ''
})

export const mutations = {
    UPDATE_SHOW_ERROR_ALERT (state, show) {
        state.showErrorAlert = show
    },
    UPDATE_ERROR_AERT_MESSAGE (state, message) {
        state.ErrorAlertMessage = message
    }
}
