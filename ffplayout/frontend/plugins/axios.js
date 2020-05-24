export default async function ({ $axios, store, redirect }) {
    await store.dispatch('auth/inspectToken')

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

    $axios.onError((error) => {
        const code = parseInt(error.response && error.response.status)

        if (code === 400) {
            redirect('/')
        }
    })
}
