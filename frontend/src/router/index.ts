import { createRouter, createWebHistory } from 'vue-router'
// import { i18n } from '../i18n'
import HomeView from '@/views/HomeView.vue'

import { useAuth } from '@/stores/auth'
import { useConfig } from '@/stores/config'

const router = createRouter({
    history: createWebHistory(import.meta.env.BASE_URL),
    routes: [
        {
            path: '/',
            name: 'home',
            component: HomeView,
        },
        {
            path: '/player',
            name: 'player',
            component: () => import('@/views/PlayerView.vue'),
        },
        {
            path: '/media',
            name: 'media',
            component: () => import('@/views/MediaView.vue'),
        },
        {
            path: '/message',
            name: 'message',
            component: () => import('@/views/MessageView.vue'),
        },
        {
            path: '/logging',
            name: 'logging',
            component: () => import('@/views/LoggingView.vue'),
        },
        {
            path: '/configure',
            name: 'configure',
            component: () => import('@/views/ConfigureView.vue'),
        },
    ],
})

router.beforeEach(async (to, from, next) => {
    const auth = useAuth()
    const configStore = useConfig()

    await configStore.configInit()

    if (!auth.isLogin && !String(to.name).includes('home')) {
        // const loc = i18n.locale.value === 'en-US' ? '' : `${i18n.locale.value}/`
        next('/')
    } else {
        next()
    }
})

export default router
