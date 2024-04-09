<template>
    <div class="navbar bg-base-100 min-h-[52px] p-0 shadow">
        <NuxtLink class="navbar-brand p-2" href="/">
            <img src="~/assets/images/ffplayout-small.png" class="img-fluid" alt="Logo" width="30" height="30" />
        </NuxtLink>
        <div class="navbar-end w-1/5 grow">
            <label class="swap swap-rotate me-2 md:hidden">
                <input type="checkbox" @change="toggleDarkTheme" :checked="indexStore.darkMode" />

                <svg class="swap-on fill-current w-5 h-5" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 22 22">
                    <path
                        d="M5.64,17l-.71.71a1,1,0,0,0,0,1.41,1,1,0,0,0,1.41,0l.71-.71A1,1,0,0,0,5.64,17ZM5,12a1,1,0,0,0-1-1H3a1,1,0,0,0,0,2H4A1,1,0,0,0,5,12Zm7-7a1,1,0,0,0,1-1V3a1,1,0,0,0-2,0V4A1,1,0,0,0,12,5ZM5.64,7.05a1,1,0,0,0,.7.29,1,1,0,0,0,.71-.29,1,1,0,0,0,0-1.41l-.71-.71A1,1,0,0,0,4.93,6.34Zm12,.29a1,1,0,0,0,.7-.29l.71-.71a1,1,0,1,0-1.41-1.41L17,5.64a1,1,0,0,0,0,1.41A1,1,0,0,0,17.66,7.34ZM21,11H20a1,1,0,0,0,0,2h1a1,1,0,0,0,0-2Zm-9,8a1,1,0,0,0-1,1v1a1,1,0,0,0,2,0V20A1,1,0,0,0,12,19ZM18.36,17A1,1,0,0,0,17,18.36l.71.71a1,1,0,0,0,1.41,0,1,1,0,0,0,0-1.41ZM12,6.5A5.5,5.5,0,1,0,17.5,12,5.51,5.51,0,0,0,12,6.5Zm0,9A3.5,3.5,0,1,1,15.5,12,3.5,3.5,0,0,1,12,15.5Z"
                    />
                </svg>

                <svg class="swap-off fill-current w-5 h-5" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 22 22">
                    <path
                        d="M21.64,13a1,1,0,0,0-1.05-.14,8.05,8.05,0,0,1-3.37.73A8.15,8.15,0,0,1,9.08,5.49a8.59,8.59,0,0,1,.25-2A1,1,0,0,0,8,2.36,10.14,10.14,0,1,0,22,14.05,1,1,0,0,0,21.64,13Zm-9.5,6.69A8.14,8.14,0,0,1,7.08,5.22v.27A10.15,10.15,0,0,0,17.22,15.63a9.79,9.79,0,0,0,2.1-.22A8.11,8.11,0,0,1,12.14,19.73Z"
                    />
                </svg>
            </label>
            <div class="dropdown dropdown-end z-50">
                <div tabindex="0" role="button" class="btn btn-ghost md:hidden">
                    <svg
                        xmlns="http://www.w3.org/2000/svg"
                        class="h-5 w-5"
                        fill="none"
                        viewBox="0 0 24 24"
                        stroke="currentColor"
                    >
                        <path
                            stroke-linecap="round"
                            stroke-linejoin="round"
                            stroke-width="2"
                            d="M4 6h16M4 12h8m-8 6h16"
                        />
                    </svg>
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
                        <button class="h-[27px] text-base" exactActiveClass="is-active" @click="logout()">Logout</button>
                    </li>
                </ul>
            </div>
        </div>
        <div class="navbar-end hidden md:flex w-4/5 min-w-[600px]">
            <ul class="menu menu-sm menu-horizontal px-1">
                <li v-for="item in menuItems" :key="item.name" class="bg-base-100 rounded-md p-0">
                    <NuxtLink :to="item.link" class="px-2 h-[27px] relative text-base text-base-content" activeClass="is-active">
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

                        <svg
                            class="swap-on fill-current w-5 h-5"
                            xmlns="http://www.w3.org/2000/svg"
                            viewBox="0 0 22 22"
                        >
                            <path
                                d="M5.64,17l-.71.71a1,1,0,0,0,0,1.41,1,1,0,0,0,1.41,0l.71-.71A1,1,0,0,0,5.64,17ZM5,12a1,1,0,0,0-1-1H3a1,1,0,0,0,0,2H4A1,1,0,0,0,5,12Zm7-7a1,1,0,0,0,1-1V3a1,1,0,0,0-2,0V4A1,1,0,0,0,12,5ZM5.64,7.05a1,1,0,0,0,.7.29,1,1,0,0,0,.71-.29,1,1,0,0,0,0-1.41l-.71-.71A1,1,0,0,0,4.93,6.34Zm12,.29a1,1,0,0,0,.7-.29l.71-.71a1,1,0,1,0-1.41-1.41L17,5.64a1,1,0,0,0,0,1.41A1,1,0,0,0,17.66,7.34ZM21,11H20a1,1,0,0,0,0,2h1a1,1,0,0,0,0-2Zm-9,8a1,1,0,0,0-1,1v1a1,1,0,0,0,2,0V20A1,1,0,0,0,12,19ZM18.36,17A1,1,0,0,0,17,18.36l.71.71a1,1,0,0,0,1.41,0,1,1,0,0,0,0-1.41ZM12,6.5A5.5,5.5,0,1,0,17.5,12,5.51,5.51,0,0,0,12,6.5Zm0,9A3.5,3.5,0,1,1,15.5,12,3.5,3.5,0,0,1,12,15.5Z"
                            />
                        </svg>

                        <svg
                            class="swap-off fill-current w-5 h-5"
                            xmlns="http://www.w3.org/2000/svg"
                            viewBox="0 0 22 22"
                        >
                            <path
                                d="M21.64,13a1,1,0,0,0-1.05-.14,8.05,8.05,0,0,1-3.37.73A8.15,8.15,0,0,1,9.08,5.49a8.59,8.59,0,0,1,.25-2A1,1,0,0,0,8,2.36,10.14,10.14,0,1,0,22,14.05,1,1,0,0,0,21.64,13Zm-9.5,6.69A8.14,8.14,0,0,1,7.08,5.22v.27A10.15,10.15,0,0,0,17.22,15.63a9.79,9.79,0,0,0,2.1-.22A8.11,8.11,0,0,1,12.14,19.73Z"
                            />
                        </svg>
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
