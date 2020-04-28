export default function ({ store, redirect }) {
    if (!store.state.auth.isLogin) {
        return redirect('/')
    }
}
