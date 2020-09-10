export default function ({ $axios, store, redirect, route }) {
    $axios.onRequest((config) => {
        const token = store.state.auth.jwtToken
        if (token) {
            config.headers.common.Authorization = `Bearer ${token}`
        }

        // disable progress on auth and stats
        if (config.url.includes('stats') || config.url.includes('auth') || config.url.includes('system')) {
            config.progress = false
        }
    })

    $axios.interceptors.response.use((response) => {
        return response
    }, (error) => {
        const originalRequest = error.config

        // prevent infinite loop
        if (error.response.status === 401 && originalRequest.url.includes('auth/refresh') && route.path !== '/') {
            store.commit('auth/REMOVE_TOKEN')
            redirect('/')
            return Promise.reject(error)
        }

        if (error.response.status === 401 && !originalRequest._retry && !originalRequest.url.includes('auth/token')) {
            originalRequest._retry = true
            return $axios.post('auth/token/refresh/', {
                refresh: store.state.auth.jwtRefresh
            })
                .then((res) => {
                    if (res.status === 201 || res.status === 200) {
                        store.commit('auth/UPADTE_TOKEN', { token: res.data.access })
                        originalRequest.headers.Authorization = `Bearer ${res.data.access}`
                        return $axios(originalRequest)
                    }
                })
        }
        return Promise.reject(error)
    })

    $axios.onError((error) => {
        const code = parseInt(error.response && error.response.status)

        if (code === 401 && route.path !== '/') {
            redirect('/')
        } else if (code !== 401) {
            store.commit('UPDATE_SHOW_ERROR_ALERT', true)
            store.commit('UPDATE_ERROR_AERT_MESSAGE', error)
        }
    })
}
