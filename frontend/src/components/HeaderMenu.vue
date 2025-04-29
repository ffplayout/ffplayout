<template>
    <div class="navbar bg-base-100 min-h-[52px] p-0 shadow-md">
        <RouterLink class="navbar-brand min-w-[46px] p-2" to="/">
            <img src="@/assets/images/ffplayout-small.png" class="img-fluid" alt="Logo" width="30" height="30" />
        </RouterLink>
        <div class="navbar-end w-1/5 grow">
            <label class="swap swap-rotate me-2 2sm:hidden">
                <input
                    type="checkbox"
                    :checked="indexStore.darkMode"
                    class="focus-within:!outline-0"
                    @change="toggleTheme"
                />
                <i class="swap-on bi bi-brightness-high text-[18px]"></i>
                <i class="swap-off bi bi-moon text-[18px]"></i>
            </label>
            <details ref="menuDropdown" tabindex="0" class="dropdown dropdown-end z-50">
                <summary class="btn btn-ghost 2sm:hidden" @click="clickMenu()" @blur="blurMenu()">
                    <SvgIcon name="burger" classes="w-5 h-5" />
                </summary>
                <ul class="menu menu-sm dropdown-content mt-1 z-[1] p-2 shadow-sm bg-base-100 rounded-box w-52">
                    <template v-for="item in menuItems" :key="item.name">
                        <li
                            v-if="
                                item.label !== 'message' ||
                                (configStore.playout?.text?.add_text && !configStore.playout?.text?.text_from_filename)
                            "
                            class="bg-base-100 rounded-md py-1"
                        >
                            <RouterLink
                                :to="item.link"
                                class="h-[27px] text-base"
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
                                <div class="h-[19px] text-base cursor-pointer">
                                    <span> {{ configStore.channels[configStore.i].name }} </span>
                                </div>
                            </summary>
                            <ul class="p-2">
                                <li v-for="(channel, index) in configStore.channels" :key="index">
                                    <span>
                                        <a class="dropdown-item cursor-pointer" @click="selectChannel(index)">{{
                                            channel.name
                                        }}</a>
                                    </span>
                                </li>
                            </ul>
                        </details>
                    </li>
                    <li class="bg-base-100 rounded-md">
                        <RouterLink
                            to="/configure"
                            class="h-[27px] leading-5"
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
                            class="h-[27px] text-base cursor-pointer"
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
        <div class="navbar-end hidden 2sm:flex w-4/5 min-w-[750px]">
            <ul class="menu menu-sm menu-horizontal px-1">
                <template v-for="item in menuItems" :key="item.name">
                    <li
                        v-if="
                            item.label !== 'message' ||
                            (configStore.playout?.text?.add_text && !configStore.playout?.text?.text_from_filename)
                        "
                        class="bg-base-100 rounded-md p-0"
                    >
                        <RouterLink
                            :to="item.link"
                            class="px-2 h-[27px] relative text-base text-base-content"
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
                            <div class="h-[19px] text-base cursor-pointer">
                                <span> {{ configStore.channels[configStore.i].name }} </span>
                            </div>
                        </summary>
                        <ul class="p-2 bg-base-100 rounded-md !mt-1 w-36" tabindex="0">
                            <li v-for="(channel, index) in configStore.channels" :key="index">
                                <a class="dropdown-item cursor-pointer" @click="selectChannel(index)">
                                    {{ channel.name }}
                                </a>
                            </li>
                        </ul>
                    </details>
                </li>
                <li class="bg-base-100 rounded-md p-0">
                    <RouterLink
                        to="/configure"
                        class="h-[27px] leading-5"
                        active-class="is-active"
                        :title="t('button.configure')"
                    >
                        <span>
                            <i class="bi bi-gear text-[17px]" />
                        </span>
                    </RouterLink>
                </li>
                <li class="p-0">
                    <label class="swap swap-rotate h-[27px] leading-5">
                        <input
                            type="checkbox"
                            :checked="indexStore.darkMode"
                            @change="toggleTheme"
                            class="focus-within:!outline-0"
                        />
                        <i class="swap-on bi bi-brightness-high text-[18px]"></i>
                        <i class="swap-off bi bi-moon text-[18px]"></i>
                    </label>
                </li>
                <li class="bg-base-100 rounded-md p-0">
                    <button class="h-[27px] leading-5 cursor-pointer" @click="logout()" :title="t('button.logout')">
                        <i class="bi bi-door-closed text-[18px]" />
                    </button>
                </li>
            </ul>
        </div>
    </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'

import SvgIcon from '@/components/SvgIcon.vue'

import { useAuth } from '@/stores/auth'
import { useIndex } from '@/stores/index'
import { useConfig } from '@/stores/config'

const { t } = useI18n()
const router = useRouter()

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

function closeDropdown($event: any) {
    setTimeout(() => {
        $event.target.parentNode?.removeAttribute('open')
    }, 200)
}

function logout() {
    authStore.removeToken()
    router.push('/')
}

function selectChannel(index: number) {
    configStore.i = index

    if (authStore.role === 'global_admin') {
        configStore.getAdvancedConfig()
    }

    configStore.getPlayoutConfig()
    configStore.getPlayoutOutputs()
}

function toggleTheme() {
    indexStore.darkMode = !indexStore.darkMode

    if (indexStore.darkMode) {
        localStorage.setItem('theme', 'dark')
        document.documentElement.setAttribute('data-theme', 'dark')
    } else {
        localStorage.setItem('theme', 'light')
        document.documentElement.setAttribute('data-theme', 'light')
    }
}
</script>
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
