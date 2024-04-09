<template>
    <div class="navbar bg-base-100 min-h-[52px] p-0 shadow">
        <NuxtLink class="navbar-brand p-2" href="/">
            <img src="~/assets/images/ffplayout-small.png" class="img-fluid" alt="Logo" width="30" height="30" />
        </NuxtLink>
        <div class="navbar-end w-1/5 grow">
            <label class="swap swap-rotate me-2 md:hidden">
                <input type="checkbox" @change="toggleDarkTheme" :checked="indexStore.darkMode" />
                <SvgIcon name="swap-on" classes="w-5 h-5" />
                <SvgIcon name="swap-off" classes="w-5 h-5" />
            </label>
            <div class="dropdown dropdown-end z-50">
                <div tabindex="0" role="button" class="btn btn-ghost md:hidden">
                    <SvgIcon name="burger" classes="w-5 h-5" />
                </div>
                <ul class="menu menu-sm dropdown-content mt-1 z-[1] p-2 shadow bg-base-100 rounded-box w-52">
                    <li v-for="item in menuItems" :key="item.name" class="bg-base-100 rounded-md">
                        <NuxtLink :to="item.link" class="h-[27px] text-base" exactActiveClass="is-active">
                            <span>
                                {{ item.name }}
                            </span>
                        </NuxtLink>
                    </li>
                    <li v-if="configStore.configGui.length > 1">
                        <details tabindex="0" @focusout="closeDropdown">
                            <summary>
                                <div class="h-[19px] text-base">
                                    <span> Channels </span>
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
                            Logout
                        </button>
                    </li>
                </ul>
            </div>
        </div>
        <div class="navbar-end hidden md:flex w-4/5 min-w-[600px]">
            <ul class="menu menu-sm menu-horizontal px-1">
                <li v-for="item in menuItems" :key="item.name" class="bg-base-100 rounded-md p-0">
                    <NuxtLink
                        :to="item.link"
                        class="px-2 h-[27px] relative text-base text-base-content"
                        activeClass="is-active"
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
                                <span> Channels </span>
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
                    <button class="h-[27px] pt-[4px] text-base" @click="logout()">Logout</button>
                </li>
                <li class="p-0">
                    <label class="swap swap-rotate">
                        <input type="checkbox" @change="toggleDarkTheme" :checked="indexStore.darkMode" />
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
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()
const router = useRouter()

const menuItems = ref([
    { name: 'Home', link: '/' },
    { name: 'Player', link: '/player' },
    { name: 'Media', link: '/media' },
    { name: 'Message', link: '/message' },
    { name: 'Logging', link: '/logging' },
    { name: 'Configure', link: '/configure' },
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
    router.push({ path: '/' })
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
