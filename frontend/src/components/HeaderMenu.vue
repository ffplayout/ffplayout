<script setup lang="ts">
import { ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'

import SvgIcon from '@/components/utils/SvgIcon.vue'

import { useAuth } from '@/stores/auth'
import { useIndex } from '@/stores/index'
import { useConfig } from '@/stores/config'

const { t } = useI18n()
const router = useRouter()
const route = useRoute()

const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()

const menuDropdown = ref()
const isOpen = ref(false)

const menuItems = ref([
    { label: 'home', name: t('button.home'), link: '/' },
    { label: 'player', name: t('button.player'), link: '/player' },
    { label: 'media', name: t('button.media'), link: '/media' },
    { label: 'message', name: t('button.message'), link: '/message' },
    { label: 'logging', name: t('button.logging'), link: '/logging' },
])

function closeMenu() {
    setTimeout(() => {
        isOpen.value = false
        menuDropdown.value?.removeAttribute('open')
    }, 200)
}

function clickMenu() {
    isOpen.value = !isOpen.value

    if (!isOpen.value) {
        menuDropdown.value?.removeAttribute('open')
    }
}

function blurMenu() {
    if (isOpen.value) {
        isOpen.value = !isOpen.value
    } else {
        setTimeout(() => {
            menuDropdown.value?.removeAttribute('open')
        }, 200)
    }
}

function closeDropdown($event: FocusEvent) {
    setTimeout(() => {
        const parent = ($event.target as HTMLElement).parentNode;
        if (parent && parent instanceof HTMLElement) {
            parent.removeAttribute('open');
        }
    }, 200)
}

async function logout() {
    await authStore.logout()
    await router.push({ name: 'login' })
}

function channelLink(path: string) {
    const channelId = configStore.channels[configStore.i]?.id

    return channelId ? { path, query: { channel: channelId } } : path
}

async function selectChannel(channelId: number) {
    await router.push({ path: route.path, query: { ...route.query, channel: channelId } })
}

function toggleTheme() {
    indexStore.darkMode = !indexStore.darkMode

    if (indexStore.darkMode) {
        localStorage.setItem('theme', 'dark')
        document.documentElement.setAttribute('data-theme', 'ffp-dark')
    } else {
        localStorage.setItem('theme', 'light')
        document.documentElement.setAttribute('data-theme', 'ffp-light')
    }
}
</script>
<template>
    <div class="navbar bg-base-100 min-h-13 p-0 shadow-md">
        <RouterLink class="navbar-brand min-w-11.5 p-2" to="/">
            <img src="@/assets/images/ffplayout-small.png" class="img-fluid" alt="Logo" width="30" height="30" />
        </RouterLink>
        <div class="navbar-end w-1/5 grow">
            <label class="swap swap-rotate me-2 2sm:hidden">
                <input
                    type="checkbox"
                    :checked="indexStore.darkMode"
                    class="focus-within:outline-0!"
                    @change="toggleTheme"
                />
                <i class="swap-on bi bi-brightness-high text-[18px]"></i>
                <i class="swap-off bi bi-moon text-[18px]"></i>
            </label>
            <details ref="menuDropdown" tabindex="0" class="dropdown dropdown-end z-50">
                <summary class="btn btn-ghost 2sm:hidden" @click="clickMenu()" @blur="blurMenu()">
                    <SvgIcon name="burger" classes="w-5 h-5" />
                </summary>
                <ul class="menu menu-sm dropdown-content mt-1 z-1 p-2 shadow-sm bg-base-100 rounded-box w-52">
                    <template v-for="item in menuItems" :key="item.name">
                        <li class="bg-base-100 rounded-md py-1">
                            <RouterLink
                                :to="channelLink(item.link)"
                                class="h-6.75 text-base"
                                exact-active-class="is-active"
                                @click="closeMenu()"
                            >
                                <span>
                                    {{ item.name }}
                                </span>
                            </RouterLink>
                        </li>
                    </template>
                    <li v-if="configStore.channels.length > 1" class="bg-base-100 rounded-md py-1">
                        <details tabindex="0" @focusout="closeDropdown">
                            <summary>
                                <div class="h-4.75 text-base cursor-pointer">
                                    <span> {{ configStore.channels[configStore.i]?.name }} </span>
                                </div>
                            </summary>
                            <ul class="p-2">
                                <li v-for="channel in configStore.channels" :key="channel.id">
                                    <span>
                                        <a class="dropdown-item cursor-pointer" @click="selectChannel(channel.id)">{{
                                            channel.name
                                        }}</a>
                                    </span>
                                </li>
                            </ul>
                        </details>
                    </li>
                    <li class="bg-base-100 rounded-md">
                        <RouterLink
                            :to="channelLink('/configure')"
                            class="h-6.75 leading-5"
                            active-class="is-active"
                            :title="t('button.configure')"
                        >
                            <span>
                                <i class="bi bi-gear text-[17px]" />
                            </span>
                        </RouterLink>
                    </li>
                    <li class="bg-base-100 rounded-md">
                        <button
                            class="h-6.75 text-base cursor-pointer"
                            exactActiveClass="is-active"
                            @click="logout()"
                            :title="t('button.logout')"
                        >
                            <i class="bi bi-door-closed text-[18px]" />
                        </button>
                    </li>
                </ul>
            </details>
        </div>
        <div class="navbar-end hidden 2sm:flex w-4/5 min-w-187.5">
            <ul class="menu menu-sm menu-horizontal px-1">
                <template v-for="item in menuItems" :key="item.name">
                    <li class="bg-base-100 rounded-md p-0">
                        <RouterLink
                            :to="channelLink(item.link)"
                            class="px-2 h-6.75 relative text-base text-base-content"
                            active-class="is-active"
                        >
                            <span>
                                {{ item.name }}
                            </span>
                        </RouterLink>
                    </li>
                </template>

                <li v-if="configStore.channels.length > 1">
                    <details tabindex="0" @focusout="closeDropdown">
                        <summary>
                            <div class="h-4.75 text-base cursor-pointer">
                                <span> {{ configStore.channels[configStore.i]?.name }} </span>
                            </div>
                        </summary>
                        <ul class="p-2 bg-base-100 rounded-md mt-1! w-36" tabindex="0">
                            <li v-for="channel in configStore.channels" :key="channel.id">
                                <a class="dropdown-item cursor-pointer" @click="selectChannel(channel.id)">
                                    {{ channel.name }}
                                </a>
                            </li>
                        </ul>
                    </details>
                </li>
                <li class="bg-base-100 rounded-md p-0">
                    <RouterLink
                        :to="channelLink('/configure')"
                        class="h-6.75 leading-5"
                        active-class="is-active"
                        :title="t('button.configure')"
                    >
                        <span>
                            <i class="bi bi-gear text-[17px]" />
                        </span>
                    </RouterLink>
                </li>
                <li class="p-0">
                    <label class="swap swap-rotate h-6.75 leading-5">
                        <input
                            type="checkbox"
                            :checked="indexStore.darkMode"
                            @change="toggleTheme"
                            class="focus-within:outline-0!"
                        />
                        <i class="swap-on bi bi-brightness-high text-[18px]"></i>
                        <i class="swap-off bi bi-moon text-[18px]"></i>
                    </label>
                </li>
                <li class="bg-base-100 rounded-md p-0">
                    <button class="h-6.75 leading-5 cursor-pointer" @click="logout()" :title="t('button.logout')">
                        <i class="bi bi-door-closed text-[18px]" />
                    </button>
                </li>
            </ul>
        </div>
    </div>
</template>
<style scoped>
.is-active > span::after {
    background: var(--my-accent);
    position: relative;
    left: 0px;
    content: ' ';
    width: inherit;
    height: 2px;
    color: red;
    display: block;
    border-radius: 0.15em;
}
</style>
