<template>
    <div class="w-full min-h-screen xs:h-full flex justify-center items-center">
        <div v-if="authStore.isLogin" class="text-center w-full max-w-[700px] p-5">
            <SystemStats v-if="configStore.configGui.length > 0" />
            <div class="flex flex-wrap justify-center gap-1 xs:gap-0 xs:join mt-5">
                <NuxtLink to="/player" class="btn join-item btn-primary">Player</NuxtLink>
                <NuxtLink to="/media" class="btn join-item btn-primary">Media</NuxtLink>
                <NuxtLink to="/message" class="btn join-item btn-primary">Message</NuxtLink>
                <NuxtLink to="logging" class="btn join-item btn-primary">Logging</NuxtLink>
                <NuxtLink
                    v-if="authStore.role.toLowerCase() == 'admin'"
                    to="/configure"
                    class="btn join-item btn-primary"
                >
                    Configure
                </NuxtLink>
                <button class="btn join-item btn-primary" @click="logout()">Logout</button>
                <label class="join-item btn btn-primary swap swap-rotate me-2">
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
            </div>
        </div>
        <div v-else class="w-96 min-w-full flex flex-col justify-center items-center px-4">
            <h1 class="text-6xl xs:text-8xl">ffplayout</h1>

            <form class="mt-10" @submit.prevent="login">
                <input
                    type="text"
                    v-model="formUsername"
                    placeholder="Username"
                    class="input input-bordered w-full"
                    required
                />

                <input
                    type="password"
                    v-model="formPassword"
                    placeholder="Password"
                    class="input input-bordered w-full mt-5"
                    required
                />

                <div class="w-full mt-4 grid grid-flow-row-dense grid-cols-12 grid-rows-1 gap-2">
                    <div class="col-span-3">
                        <button type="submit" class="btn btn-primary">Login</button>
                    </div>
                    <div class="col-span-12 sm:col-span-9">
                        <div
                            v-if="showLoginError"
                            role="alert"
                            class="alert error w-auto rounded z-2 h-12 p-[0.7rem]"
                        >
                            <svg
                                xmlns="http://www.w3.org/2000/svg"
                                class="stroke-current shrink-0 h-6 w-6"
                                fill="none"
                                viewBox="0 0 24 24"
                            >
                                <path
                                    stroke-linecap="round"
                                    stroke-linejoin="round"
                                    stroke-width="2"
                                    d="M10 14l2-2m0 0l2-2m-2 2l-2-2m2 2l2 2m7-2a9 9 0 11-18 0 9 9 0 0118 0z"
                                />
                            </svg>
                            <span>{{ formError }}</span>
                        </div>
                    </div>
                </div>
            </form>
        </div>
    </div>
</template>

<script setup lang="ts">
const colorMode = useColorMode()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()

const formError = ref('')
const showLoginError = ref(false)
const formUsername = ref('')
const formPassword = ref('')

authStore.inspectToken()

async function login() {
    try {
        const status = await authStore.obtainToken(formUsername.value, formPassword.value)

        formUsername.value = ''
        formPassword.value = ''
        formError.value = ''

        if (status === 401 || status === 400 || status === 403) {
            formError.value = 'Wrong User/Password!'
            showLoginError.value = true

            setTimeout(() => {
                showLoginError.value = false
            }, 3000)
        }

        await configStore.nuxtClientInit()
    } catch (e) {
        formError.value = e as string
    }
}

function toggleDarkTheme() {
    indexStore.darkMode = !indexStore.darkMode

    if (indexStore.darkMode) {
        colorMode.preference = 'dark'
    } else {
        colorMode.preference = 'light'
    }
}

async function logout() {
    try {
        authStore.removeToken()
    } catch (e) {
        formError.value = e as string
    }
}
</script>
