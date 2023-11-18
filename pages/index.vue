<template>
    <div>
        <div v-if="authStore.isLogin">
            <div class="container login-container">
                <div>
                    <SystemStats v-if="configStore.configGui.length > 0" class="mx-auto" />
                    <div class="text-center mt-5">
                        <div class="btn-group actions-grp btn-group-lg" role="group">
                            <NuxtLink to="/player" class="btn btn-primary">Player</NuxtLink>
                            <NuxtLink to="/media" class="btn btn-primary">Media</NuxtLink>
                            <NuxtLink to="/message" class="btn btn-primary">Message</NuxtLink>
                            <NuxtLink to="logging" class="btn btn-primary">Logging</NuxtLink>
                            <NuxtLink v-if="authStore.role.toLowerCase() == 'admin'" to="/configure" class="btn btn-primary"> Configure </NuxtLink>
                            <button class="btn btn-primary" @click="logout()">Logout</button>
                        </div>
                    </div>
                </div>
            </div>
        </div>
        <div v-else>
            <div class="logout-div" />
            <div class="container login-container">
                <div>
                    <div class="text-center mb-5">
                        <h1>ffplayout</h1>
                    </div>

                    <form class="login-form" @submit.prevent="login">
                        <div id="input-group-1" class="mb-3">
                            <label for="input-user" class="form-label">User:</label>
                            <input
                                type="text"
                                id="input-user"
                                class="form-control"
                                v-model="formUsername"
                                aria-describedby="Username"
                                required
                            />
                        </div>
                        <div class="mb-3">
                            <label for="input-pass" class="form-label">Password:</label>
                            <input
                                type="password"
                                id="input-pass"
                                class="form-control"
                                v-model="formPassword"
                                required
                            />
                        </div>
                        <div class="row">
                            <div class="col-3">
                                <button class="btn btn-primary" type="submit">Login</button>
                            </div>
                            <div class="col-9">
                                <div
                                    class="alert alert-danger alert-dismissible fade login-alert"
                                    :class="{ show: showError }"
                                    role="alert"
                                >
                                    {{ formError }}
                                    <button
                                        type="button"
                                        class="btn-close"
                                        data-bs-dismiss="alert"
                                        aria-label="Close"
                                    ></button>
                                </div>
                            </div>
                        </div>
                    </form>
                </div>
            </div>
        </div>
    </div>
</template>

<script setup lang="ts">
import { useAuth } from '~/stores/auth'
import { useConfig } from '~/stores/config'

const authStore = useAuth()
const configStore = useConfig()

const formError = ref('')
const showError = ref(false)
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
            showError.value = true
        }

        await configStore.nuxtClientInit()
    } catch (e) {
        formError.value = e as string
    }
}

async function logout() {
    try {
        authStore.removeToken()
        authStore.isLogin = false
    } catch (e) {
        formError.value = e as string
    }
}
</script>

<style lang="scss">
.login-container {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100vh;
}

.login-form {
    min-width: 300px;
}

.login-alert {
    padding: 0.4em;
    --bs-alert-margin-bottom: 0;

    .btn-close {
        padding: 0.65rem 0.5rem;
    }
}

@media (max-width: 380px) {
    .actions-grp {
        display: flex;
        flex-direction: column;
        margin: 0 2em 0 2em;
    }
}
</style>
