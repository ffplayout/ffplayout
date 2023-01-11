<template>
    <div>
        <div class="container">
            <h2 class="pb-4 pt-3">User Configuration</h2>
            <form v-if="configStore.configUser" @submit.prevent="onSubmitUser">
                <div class="mb-3 row">
                    <label for="userName" class="col-sm-2 col-form-label">Username</label>
                    <div class="col-sm-10">
                        <input
                            type="text"
                            class="form-control"
                            id="userName"
                            v-model="configStore.configUser.username"
                            disabled
                        />
                    </div>
                </div>
                <div class="mb-3 row">
                    <label for="userMail" class="col-sm-2 col-form-label">mail</label>
                    <div class="col-sm-10">
                        <input type="text" class="form-control" id="userMail" v-model="configStore.configUser.mail" />
                    </div>
                </div>
                <div class="mb-3 row">
                    <label for="userPass1" class="col-sm-2 col-form-label">New Password</label>
                    <div class="col-sm-10">
                        <input type="password" class="form-control" id="userPass1" v-model="newPass" />
                    </div>
                </div>
                <div class="mb-3 row">
                    <label for="userPass2" class="col-sm-2 col-form-label">Confirm Password</label>
                    <div class="col-sm-10">
                        <input type="password" class="form-control" id="userPass2" v-model="confirmPass" />
                    </div>
                </div>
                <div class="row">
                    <div class="col-1" style="min-width: 85px">
                        <button class="btn btn-primary" type="submit">Save</button>
                    </div>
                </div>
            </form>
        </div>
    </div>
</template>

<script setup lang="ts">
import { useAuth } from '~/stores/auth'
import { useConfig } from '~/stores/config'
import { useIndex } from '~/stores/index'

const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()

const newPass = ref('')
const confirmPass = ref('')

async function onSubmitUser() {
    if (newPass && newPass.value === confirmPass.value) {
        configStore.configUser.password = newPass.value
    }

    authStore.inspectToken()
    const update = await configStore.setUserConfig(configStore.configUser)

    if (update.status === 200) {
        indexStore.alertVariant = 'alert-success'
        indexStore.alertMsg = 'Update user profile success!'
    } else {
        indexStore.alertVariant = 'alert-danger'
        indexStore.alertMsg = 'Update user profile failed!'
    }

    indexStore.showAlert = true

    newPass.value = ''
    confirmPass.value = ''

    setTimeout(() => {
        indexStore.showAlert = false
    }, 2000)
}
</script>
