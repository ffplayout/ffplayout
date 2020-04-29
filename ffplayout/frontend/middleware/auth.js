export default async function ({ store, redirect }) {
    await store.dispatch('auth/inspectToken')

    if (!store.state.auth.isLogin) {
        return redirect('/')
    }
}
