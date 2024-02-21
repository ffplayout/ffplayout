export default defineNuxtRouteMiddleware((to, from) => {
    const auth = useAuth()

    auth.inspectToken()

    if (!auth.isLogin && to.path  !== '/') {
        return navigateTo('/')
    }
})
