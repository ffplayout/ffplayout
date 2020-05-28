<template>
    <div>
        <nuxt />
        <b-alert
            :show="showErrorAlert"
            dismissible
            variant="danger"
            class="status-alert"
            @dismissed="showErrorAlert=!showErrorAlert"
        >
            <p>{{ ErrorAlertMessage }}</p>
        </b-alert>
    </div>
</template>

<script>
import { mapState } from 'vuex'

export default {
    computed: {
        ...mapState(['ErrorAlertMessage']),

        showErrorAlert: {
            get () {
                return this.$store.state.showErrorAlert
            },
            set () {
                this.$store.commit('UPDATE_SHOW_ERROR_ALERT', false)
            }
        }
    }
}
</script>

<style>
html, body {
    font-family: 'Source Sans Pro', -apple-system, BlinkMacSystemFont, 'Segoe UI',
        Roboto, 'Helvetica Neue', Arial, sans-serif;
    color: #c4c4c4;
    font-size: 15px;
    word-spacing: 1px;
    -ms-text-size-adjust: 100%;
    -webkit-text-size-adjust: 100%;
    -moz-osx-font-smoothing: grayscale;
    -webkit-font-smoothing: antialiased;
    box-sizing: border-box;
    height: 100%;
    padding: 0;
    margin: 0;
}

a {
    color: #c4c4c4;
}

.status-alert {
    position: fixed;
    z-index: 1001;
    top: 55px;
    right: 10px;
    width: 400px;
    max-width: 500px;
    height: 100px;
    max-height: 100px;
}
</style>
