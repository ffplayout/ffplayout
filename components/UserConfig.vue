<template>
    <div>
        <div class="container">
            <h2 class="pb-4 pt-3">User Configuration</h2>
            <div class="row w-100" style="height: 43px">
                <div class="col-sm-2"></div>
                <div class="col-sm-4">
                    <select class="form-select" v-model="selected" @change="onChange($event)">
                        <option v-for="item in users">{{ item.username }}</option>
                    </select>
                </div>
                <div class="col px-0">
                    <div class="float-end">
                        <button
                            class="btn btn-primary me-2"
                            title="Add new User"
                            data-tooltip="tooltip"
                            data-bs-toggle="modal"
                            data-bs-target="#userModal"
                        >
                            Add User
                        </button>
                        <button class="btn btn-primary" @click="deleteUser()">Delete</button>
                    </div>
                </div>
            </div>
            <form v-if="configStore.configUser" @submit.prevent="onSubmitUser">
                <div class="mb-3 row">
                    <label for="userName" class="col-sm-2 col-form-label">Username</label>
                    <div class="col-sm-10">
                        <input
                            type="text"
                            class="form-control"
                            id="userName"
                            v-model="configStore.configUser.username"
                        />
                    </div>
                </div>
                <div class="mb-3 row">
                    <label for="userMail" class="col-sm-2 col-form-label">mail</label>
                    <div class="col-sm-10">
                        <input type="email" class="form-control" id="userMail" v-model="configStore.configUser.mail" />
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

        <div
            id="userModal"
            ref="userModal"
            class="modal"
            tabindex="-1"
            aria-labelledby="userModalLabel"
            aria-hidden="true"
        >
            <div class="modal-dialog modal-dialog-centered">
                <div class="modal-content">
                    <div class="modal-header">
                        <h1 class="modal-title fs-5" id="userModalLabel">Add User</h1>
                        <button type="button" class="btn-close" data-bs-dismiss="modal" aria-label="Cancel"></button>
                    </div>
                    <form @reset="clearUser" @submit.prevent="addUser">
                        <div class="modal-body">
                            <label for="name-input" class="form-label">Username</label>
                            <input
                                type="text"
                                class="form-control"
                                id="name-input"
                                aria-describedby="Username"
                                v-model.number="user.username"
                                required
                            />
                            <label for="mail-input" class="form-label mt-2">Mail</label>
                            <input
                                type="email"
                                class="form-control"
                                id="mail-input"
                                aria-describedby="Mail Address"
                                v-model.number="user.mail"
                                required
                            />
                            <label for="pass-input" class="form-label mt-2">Password</label>
                            <input
                                type="password"
                                class="form-control"
                                id="pass-input"
                                aria-describedby="Password"
                                v-model.string="user.password"
                                required
                            />
                            <label for="confirm-input" class="form-label mt-2">Confirm Password</label>
                            <input
                                type="password"
                                class="form-control"
                                id="confirm-input"
                                aria-describedby="Confirm Password"
                                v-model.string="user.confirm"
                                required
                            />
                            <div class="form-check mt-3">
                                <input
                                    class="form-check-input"
                                    type="checkbox"
                                    id="isAdmin"
                                    v-model.number="user.admin"
                                />
                                <label class="form-check-label" for="isAdmin">Admin</label>
                            </div>
                        </div>
                        <div class="modal-footer">
                            <button type="reset" class="btn btn-primary" data-bs-dismiss="modal" aria-label="Cancel">
                                Cancel
                            </button>
                            <button type="submit" class="btn btn-primary">Ok</button>
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
import { useIndex } from '~/stores/index'

const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()

const { $bootstrap } = useNuxtApp()

const selected = ref(null)
const users = ref([] as User[])
const userModal = ref()
const newPass = ref('')
const confirmPass = ref('')

const user = ref({
    username: '',
    mail: '',
    password: '',
    confirm: '',
    admin: false,
    role_id: 2,
} as User)

onMounted(() => {
    getUsers()
})

async function getUsers() {
    fetch('/api/users', {
        method: 'GET',
        headers: authStore.authHeader,
    })
        .then((response) => response.json())
        .then((data) => {
            users.value = data
        })
}

function onChange(event: any) {
    selected.value = event.target.value

    getUserConfig()
}

async function getUserConfig() {
    await fetch(`/api/user/${selected.value}`, {
        method: 'GET',
        headers: authStore.authHeader,
    })
        .then((response) => response.json())
        .then((data) => {
            configStore.configUser = data
        })
}

async function deleteUser() {
    if (configStore.configUser.username === configStore.currentUser) {
        indexStore.alertVariant = 'alert-danger'
        indexStore.alertMsg = 'Delete current user not possible!'


    } else {
        await fetch(`/api/user/${configStore.configUser.username}`, {
            method: 'DELETE',
            headers: authStore.authHeader,
        }).then(async () => {
            indexStore.alertVariant = 'alert-success'
            indexStore.alertMsg = 'Delete user done!'

            await configStore.getUserConfig()
            await getUsers()
        })
        .catch((e) => {
            indexStore.alertVariant = 'alert-danger'
            indexStore.alertMsg = `Delete user error: ${e}`
        })
    }

    indexStore.showAlert = true

    setTimeout(() => {
            indexStore.showAlert = false
        }, 3000)
}

async function clearUser() {
    user.value.username = ''
    user.value.mail = ''
    user.value.password = ''
    user.value.confirm = ''
    user.value.admin = false
    user.value.role_id = 2
}

async function addUser() {
    if (user.value.admin) {
        user.value.role_id = 1
    } else {
        user.value.role_id = 2
    }

    delete user.value.admin

    if (user.value.password === user.value.confirm) {
        // @ts-ignore
        const modal = $bootstrap.Modal.getOrCreateInstance(userModal.value)
        modal.hide()

        authStore.inspectToken()
        const update = await configStore.addNewUser(user.value)

        if (update.status === 200) {
            indexStore.alertVariant = 'alert-success'
            indexStore.alertMsg = 'Add user success!'

            await getUsers()
            await getUserConfig()
        } else {
            indexStore.alertVariant = 'alert-danger'
            indexStore.alertMsg = 'Add user failed!'
        }

        clearUser()
    } else {
        indexStore.alertVariant = 'alert-danger'
        indexStore.alertMsg = 'Password mismatch!'
    }

    indexStore.showAlert = true

    setTimeout(() => {
        indexStore.showAlert = false
    }, 2000)
}

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
