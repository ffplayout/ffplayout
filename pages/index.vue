<template>
    <div class="w-full min-h-screen xs:h-full flex justify-center items-center">
        <div v-if="authStore.isLogin" class="flex flex-wrap justify-center text-center w-full max-w-[1024px] p-5">
            <div class="absolute top-4 left-1">
                <EventStatus />
            </div>
            <SystemStats v-if="configStore.configGui.length > 0" />
            <div class="flex flex-wrap justify-center gap-1 md:gap-0 md:join mt-5">
                <NuxtLink :to="localePath({ name: 'player' })" class="btn join-item btn-primary px-2">
                    {{ $t('button.player') }}
                </NuxtLink>
                <NuxtLink :to="localePath({ name: 'media' })" class="btn join-item btn-primary px-2">
                    {{ $t('button.media') }}
                </NuxtLink>
                <NuxtLink :to="localePath({ name: 'message' })" class="btn join-item btn-primary px-2">
                    {{ $t('button.message') }}
                </NuxtLink>
                <NuxtLink :to="localePath({ name: 'logging' })" class="btn join-item btn-primary px-2">
                    {{ $t('button.logging') }}
                </NuxtLink>
                <NuxtLink
                    v-if="authStore.role.toLowerCase() == 'admin'"
                    :to="localePath({ name: 'configure' })"
                    class="btn join-item btn-primary px-2"
                >
                    {{ $t('button.configure') }}
                </NuxtLink>
                <button class="btn join-item btn-primary px-2" @click="logout()">
                    {{ $t('button.logout') }}
                </button>
                <select
                    v-model="selectedLang"
                    class="select select-primary select-bordered join-item max-w-xs ps-2"
                    @change="changeLang(selectedLang)"
                >
                    <option v-for="(loc, index) in locales" :key="index" :value="/* @ts-ignore */ loc.code">
                        {{
                            /* @ts-ignore */
                            loc.name
                        }}
                    </option>
                </select>
                <label class="join-item btn btn-primary swap swap-rotate me-2">
                    <input type="checkbox" :checked="indexStore.darkMode" @change="toggleDarkTheme" />
                    <SvgIcon name="swap-on" classes="w-5 h-5" />
                    <SvgIcon name="swap-off" classes="w-5 h-5" />
                </label>
            </div>
        </div>
        <div v-else class="w-96 min-w-full flex flex-col justify-center items-center px-4">
            <h1 class="text-6xl xs:text-8xl">ffplayout</h1>

            <form class="mt-10" @submit.prevent="login">
                <input
                    v-model="formUsername"
                    type="text"
                    :placeholder="$t('input.username')"
                    class="input input-bordered w-full"
                    required
                />

                <input
                    v-model="formPassword"
                    type="password"
                    :placeholder="$t('input.password')"
                    class="input input-bordered w-full mt-5"
                    required
                />

                <div class="w-full mt-4 grid grid-flow-row-dense grid-cols-12 grid-rows-1 gap-2">
                    <div class="col-span-3">
                        <button type="submit" class="btn btn-primary">
                            {{ $t('button.login') }}
                        </button>
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
const { locale, locales, t } = useI18n()
const localePath = useLocalePath()
const switchLocalePath = useSwitchLocalePath()
const router = useRouter()

const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()

const selectedLang = ref(locale)
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
            formError.value = t('alert.wrongLogin')
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

async function changeLang(code: string) {
    const path = switchLocalePath(code)
    const cookie = useCookie('i18n_redirected')
    cookie.value = code

    router.push(path)
}
</script>
