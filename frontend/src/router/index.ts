import { createRouter, createWebHistory } from 'vue-router'
// import { i18n } from '../i18n'
import HomeView from '@/views/HomeView.vue'
import LoginView from '@/views/LoginView.vue'

import { useAuth } from '@/stores/auth'
import { useConfig } from '@/stores/config'

const router = createRouter({
    history: createWebHistory(import.meta.env.BASE_URL),
    routes: [
        {
            path: '/login',
            name: 'login',
            component: LoginView,
            meta: { public: true, showHeader: false },
        },
        {
            path: '/',
            name: 'home',
            component: HomeView,
            meta: { showHeader: false },
        },
        {
            path: '/verification',
            name: 'verification',
            component: () => import('@/views/VerificationView.vue'),
            meta: { public: true, showHeader: false },
        },
        {
            path: '/init',
            name: 'init',
            component: () => import('@/views/InitView.vue'),
            meta: { public: true, showHeader: false },
        },
        {
            path: '/player',
            name: 'player',
            component: () => import('@/views/PlayerView.vue'),
            meta: { showHeader: true },
        },
        {
            path: '/media',
            name: 'media',
            component: () => import('@/views/MediaView.vue'),
            meta: { showHeader: true },
        },
        {
            path: '/message',
            name: 'message',
            component: () => import('@/views/MessageView.vue'),
            meta: { showHeader: true },
        },
        {
            path: '/logging',
            name: 'logging',
            component: () => import('@/views/LoggingView.vue'),
            meta: { showHeader: true },
        },
        {
            path: '/configure',
            name: 'configure',
            component: () => import('@/views/ConfigureView.vue'),
            meta: { showHeader: true },
            redirect: { name: 'configure-channel' },
            children: [
                {
                    path: 'channel',
                    name: 'configure-channel',
                    component: () => import('@/components/config/ConfigChannel.vue'),
                    meta: { showHeader: true, roles: ['global_admin', 'channel_admin', 'user'] },
                },
                {
                    path: 'playout',
                    name: 'configure-playout',
                    component: () => import('@/components/config/ConfigPlayout.vue'),
                    meta: { showHeader: true, roles: ['global_admin', 'channel_admin'] },
                },
                {
                    path: 'user',
                    name: 'configure-user',
                    component: () => import('@/components/config/ConfigUser.vue'),
                    meta: { showHeader: true, roles: ['global_admin', 'channel_admin', 'user'] },
                },
                {
                    path: 'global',
                    name: 'configure-global',
                    component: () => import('@/components/config/ConfigGlobal.vue'),
                    meta: { showHeader: true, roles: ['global_admin'] },
                },
            ],
        },
    ],
})

router.beforeEach(async (to) => {
    const auth = useAuth()
    const configStore = useConfig()

    if (to.name === 'home') {
        const setupResponse = await fetch('/api/setup').catch(() => undefined)
        if (setupResponse?.ok && (await setupResponse.json()).required) {
            return { name: 'init' }
        }
    }

    await auth.inspectToken()

    const isVerificationRoute = to.name === 'verification'
    const isPublicRoute = to.meta.public === true

    if (isVerificationRoute) {
        if (auth.isLogin) {
            return { name: 'home' }
        }
        if (!auth.verificationPending) {
            return { name: 'login' }
        }
        return
    }

    if (!auth.isLogin && !isPublicRoute) {
        // const loc = i18n.locale.value === 'en-US' ? '' : `${i18n.locale.value}/`
        return { name: 'login' }
    }

    if (auth.isLogin && to.name === 'login') {
        return { name: 'home' }
    }

    if (!isPublicRoute) {
        const channelQuery = Array.isArray(to.query.channel) ? to.query.channel[0] : to.query.channel
        const channelId = channelQuery ? Number(channelQuery) : undefined
        await configStore.configInit(Number.isSafeInteger(channelId) ? channelId : undefined)
    }

    const allowedRoles = to.meta.roles as string[] | undefined

    if (allowedRoles && !allowedRoles.includes(auth.role)) {
        return { name: 'configure-channel' }
    }

    return
})

export default router
