export default async (context) => {
    await context.store.dispatch('config/nuxtClientInit', context)
}
