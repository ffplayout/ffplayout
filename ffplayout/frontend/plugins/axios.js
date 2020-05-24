export default function ({ $axios, store, redirect }) {
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
        if (error.response.status === 401 && originalRequest.url.includes('auth/refresh/refresh')) {
            store.commit('auth/REMOVE_TOKEN')
            redirect('/')
            return Promise.reject(error)
        }

        if (error.response.status === 401 && !originalRequest._retry) {
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

        if (code === 400) {
            redirect('/')
        }
    })
}
