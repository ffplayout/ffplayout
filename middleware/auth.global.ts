export default defineNuxtRouteMiddleware((to) => {
    const auth = useAuth()
    const localePath = useLocalePath()

    auth.inspectToken()

    if (!auth.isLogin && !String(to.name).includes('index_')) {
        return navigateTo(localePath({ name: 'index' }))
    }
})
