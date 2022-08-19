export default function ({ $axios, store, redirect, route }) {
    $axios.onRequest((config) => {
        const token = store.state.auth.jwtToken
        if (token) {
            config.headers.common.Authorization = `Bearer ${token}`
        }

        // disable progress on auth
        if (config.url.includes('auth') || config.url.includes('process') || config.url.includes('current')) {
            config.progress = false
        }
    })

    $axios.interceptors.response.use((response) => {
        return response
    }, (error) => {
        const originalRequest = error.config

        // prevent infinite loop
        if (error.response.status === 401 && route.path !== '/') {
            store.commit('auth/REMOVE_TOKEN')
            redirect('/')
            return Promise.reject(error)
        }

        if (error.response.status === 401 && !originalRequest._retry && !originalRequest.url.includes('auth/token')) {
            originalRequest._retry = true

            store.commit('auth/REMOVE_TOKEN')
            store.commit('auth/UPDATE_IS_LOGIN', false)
            redirect('/')
        }
        return Promise.reject(error)
    })

    $axios.onError((error) => {
        const code = parseInt(error.response && error.response.status)

        if (code === 401 && route.path !== '/') {
            redirect('/')
        } else if (code !== 401 && code !== 409 && code !== 503) {
            store.commit('UPDATE_VARIANT', 'danger')
            store.commit('UPDATE_SHOW_ERROR_ALERT', true)
            store.commit('UPDATE_ERROR_ALERT_MESSAGE', error)
        }

        return error.response
    })
}
