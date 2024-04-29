<template>
    <div class="navbar bg-base-100 min-h-[52px] p-0 shadow">
        <NuxtLink class="navbar-brand min-w-[46px] p-2" href="/">
            <img src="~/assets/images/ffplayout-small.png" class="img-fluid" alt="Logo" width="30" height="30" />
        </NuxtLink>
        <EventStatus v-if="route.name?.toString().includes('player__')" class="z-10"/>
        <div class="navbar-end w-1/5 grow">
            <label class="swap swap-rotate me-2 md:hidden">
                <input type="checkbox" :checked="indexStore.darkMode" @change="toggleDarkTheme" />
                <SvgIcon name="swap-on" classes="w-5 h-5" />
                <SvgIcon name="swap-off" classes="w-5 h-5" />
            </label>
            <div class="dropdown dropdown-end z-50">
                <div tabindex="0" role="button" class="btn btn-ghost md:hidden">
                    <SvgIcon name="burger" classes="w-5 h-5" />
                </div>
                <ul class="menu menu-sm dropdown-content mt-1 z-[1] p-2 shadow bg-base-100 rounded-box w-52">
                    <li v-for="item in menuItems" :key="item.name" class="bg-base-100 rounded-md">
                        <NuxtLink :to="item.link" class="h-[27px] text-base" exact-active-class="is-active">
                            <span>
                                {{ item.name }}
                            </span>
                        </NuxtLink>
                    </li>
                    <li v-if="configStore.configGui.length > 1">
                        <details tabindex="0" @focusout="closeDropdown">
                            <summary>
                                <div class="h-[19px] text-base">
                                    <span> {{ $t('button.channels') }} </span>
                                </div>
                            </summary>
                            <ul class="p-2">
                                <li v-for="(channel, index) in configStore.configGui" :key="index">
                                    <span>
                                        <a class="dropdown-item" @click="selectChannel(index)">{{ channel.name }}</a>
                                    </span>
                                </li>
                            </ul>
                        </details>
                    </li>
                    <li class="bg-base-100 rounded-md">
                        <button class="h-[27px] text-base" exactActiveClass="is-active" @click="logout()">
                            {{ $t('button.logout') }}
                        </button>
                    </li>
                </ul>
            </div>
        </div>
        <div class="navbar-end hidden md:flex w-4/5 min-w-[750px]">
            <ul class="menu menu-sm menu-horizontal px-1">
                <li v-for="item in menuItems" :key="item.name" class="bg-base-100 rounded-md p-0">
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
                <li v-if="configStore.configGui.length > 1">
                    <details tabindex="0" @focusout="closeDropdown">
                        <summary>
                            <div class="h-[19px] text-base">
                                <span> {{ $t('button.channels') }} </span>
                            </div>
                        </summary>
                        <ul class="p-2 bg-base-100 rounded-md !mt-1 w-36" tabindex="0">
                            <li v-for="(channel, index) in configStore.configGui" :key="index">
                                <a class="dropdown-item" @click="selectChannel(index)">
                                    {{ channel.name }}
                                </a>
                            </li>
                        </ul>
                    </details>
                </li>
                <li class="bg-base-100 rounded-md p-0">
                    <button class="h-[27px] pt-[4px] text-base" @click="logout()">
                        {{ $t('button.logout') }}
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

const menuItems = ref([
    { name: t('button.home'), link: localePath({ name: 'index' }) },
    { name: t('button.player'), link: localePath({ name: 'player' }) },
    { name: t('button.media'), link: localePath({ name: 'media' }) },
    { name: t('button.message'), link: localePath({ name: 'message' }) },
    { name: t('button.logging'), link: localePath({ name: 'logging' }) },
    { name: t('button.configure'), link: localePath({ name: 'configure' }) },
])

if (colorMode.value === 'dark') {
    indexStore.darkMode = true
}

function closeDropdown($event: any) {
    setTimeout(() => {
        $event.target.parentNode.removeAttribute('open')
    }, 200)
}

function logout() {
    authStore.removeToken()
    router.push(localePath({ name: 'index' }))
}

function selectChannel(index: number) {
    configStore.configID = index
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
