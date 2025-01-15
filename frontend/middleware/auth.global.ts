export default defineNuxtRouteMiddleware(async (to) => {
    const auth = useAuth()
    const localePath = useLocalePath()

    await auth.inspectToken()

    if (!auth.isLogin && !String(to.name).includes('index_')) {
        return navigateTo(localePath({ name: 'index' }))
    }
})
