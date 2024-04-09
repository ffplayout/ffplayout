<template>
    <div class="w-full min-h-screen xs:h-full flex justify-center items-center">
        <div v-if="authStore.isLogin" class="text-center w-full max-w-[700px] p-5">
            <SystemStats v-if="configStore.configGui.length > 0" />
            <div class="flex flex-wrap justify-center gap-1 xs:gap-0 xs:join mt-5">
                <NuxtLink to="/player" class="btn join-item btn-primary">Player</NuxtLink>
                <NuxtLink to="/media" class="btn join-item btn-primary">Media</NuxtLink>
                <NuxtLink to="/message" class="btn join-item btn-primary">Message</NuxtLink>
                <NuxtLink to="/logging" class="btn join-item btn-primary">Logging</NuxtLink>
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
                    <SvgIcon name="swap-on" classes="w-5 h-5" />
                    <SvgIcon name="swap-off" classes="w-5 h-5" />
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
                            class="alert alert-error w-auto rounded z-2 h-12 p-[0.7rem]"
                        >
                            <SvgIcon name="error" />
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
