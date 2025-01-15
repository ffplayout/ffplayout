<template>
    <div class="navbar bg-base-100 min-h-[52px] p-0 shadow-md">
        <NuxtLink class="navbar-brand min-w-[46px] p-2" href="/">
            <img src="~/assets/images/ffplayout-small.png" class="img-fluid" alt="Logo" width="30" height="30" />
        </NuxtLink>
        <div class="navbar-end w-1/5 grow">
            <label class="swap swap-rotate me-2 2sm:hidden">
                <input type="checkbox" :checked="indexStore.darkMode" @change="toggleDarkTheme" />
                <SvgIcon name="swap-on" classes="w-5 h-5" />
                <SvgIcon name="swap-off" classes="w-5 h-5" />
            </label>
            <details ref="menuDropdown" tabindex="0" class="dropdown dropdown-end z-50">
                <summary class="btn btn-ghost 2sm:hidden" @click="clickMenu()" @blur="blurMenu()">
                    <SvgIcon name="burger" classes="w-5 h-5" />
                </summary>
                <ul class="menu menu-sm dropdown-content mt-1 z-[1] p-2 shadow bg-base-100 rounded-box w-52">
                    <template v-for="item in menuItems" :key="item.name">
                        <li
                            v-if="
                                item.label !== 'message' ||
                                (configStore.playout?.text?.add_text && !configStore.playout?.text?.text_from_filename)
                            "
                            class="bg-base-100 rounded-md"
                        >
                            <NuxtLink
                                :to="item.link"
                                class="h-[27px] text-base"
                                exact-active-class="is-active"
                                @click="closeMenu()"
                            >
                                <span>
                                    {{ item.name }}
                                </span>
                            </NuxtLink>
                        </li>
                    </template>
                    <li v-if="configStore.channels.length > 1">
                        <details tabindex="0" @focusout="closeDropdown">
                            <summary>
                                <div class="h-[19px] text-base">
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
                        <button class="h-[27px] text-base" exactActiveClass="is-active" @click="logout()">
                            {{ t('button.logout') }}
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
                        <NuxtLink
                            :to="item.link"
                            class="px-2 h-[27px] relative text-base text-base-content"
                            active-class="is-active"
                        >
                            <span>
                                {{ item.name }}
                            </span>
                        </NuxtLink>
                    </li>
                </template>

                <li v-if="configStore.channels.length > 1">
                    <details tabindex="0" @focusout="closeDropdown">
                        <summary>
                            <div class="h-[19px] text-base">
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
                    <button class="h-[27px] pt-[4px] text-base" @click="logout()">
                        {{ t('button.logout') }}
                    </button>
                </li>
                <li class="p-0">
                    <label class="swap swap-rotate">
                        <input type="checkbox" :checked="indexStore.darkMode" @change="toggleDarkTheme" />
                        <SvgIcon name="swap-on" classes="w-5 h-5" />
                        <SvgIcon name="swap-off" classes="w-5 h-5" />
                    </label>
                </li>
            </ul>
        </div>
    </div>
</template>

<script setup lang="ts">
const colorMode = useColorMode()
const { t } = useI18n()
const localePath = useLocalePath()
const route = useRoute()
const router = useRouter()

const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()

const menuDropdown = ref()
const isOpen = ref(false)

const menuItems = ref([
    { label: 'index', name: t('button.home'), link: localePath({ name: 'index' }) },
    { label: 'player', name: t('button.player'), link: localePath({ name: 'player' }) },
    { label: 'media', name: t('button.media'), link: localePath({ name: 'media' }) },
    { label: 'message', name: t('button.message'), link: localePath({ name: 'message' }) },
    { label: 'logging', name: t('button.logging'), link: localePath({ name: 'logging' }) },
    { label: 'configure', name: t('button.configure'), link: localePath({ name: 'configure' }) },
])

if (colorMode.value === 'dark') {
    indexStore.darkMode = true
}

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
    router.push(localePath({ name: 'index' }))
}

function selectChannel(index: number) {
    configStore.i = index

    if (authStore.role === 'global_admin') {
        configStore.getAdvancedConfig()
    }

    configStore.getPlayoutConfig()
}

function toggleDarkTheme() {
    indexStore.darkMode = !indexStore.darkMode

    if (indexStore.darkMode) {
        colorMode.preference = 'dark'
    } else {
        colorMode.preference = 'light'
    }
}
</script>
<style lang="scss" scoped>
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
