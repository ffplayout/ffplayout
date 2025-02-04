<template>
    <div class="w-full max-w-[800px] xs:pe-8">
        <h2 class="pt-3 text-3xl">{{ t('user.title') }}</h2>
        <div v-if="authStore.role === 'global_admin'" class="w-full join max-w-md mt-10">
            <select v-model="selected" class="join-item select select-bordered w-full" @change="onChange($event)">
                <option v-for="item in users" :key="item.username" :value="item.id">{{ item.username }}</option>
            </select>
            <button
                class="join-item btn btn-primary select-bordered"
                title="Add new User"
                @click="showUserModal = true"
            >
                <i class="bi-plus-lg" />
            </button>
            <button
                class="join-item btn btn-primary select-bordered"
                title="Delete selected user"
                @click="deleteUser()"
            >
                <i class="bi-x-lg" />
            </button>
        </div>
        <form v-if="configStore.configUser" class="mt-5" @submit.prevent="onSubmitUser">
            <label class="form-control w-full max-w-md">
                <div class="label">
                    <span class="label-text">{{ t('user.name') }}</span>
                </div>
                <input
                    v-model="configStore.configUser.username"
                    type="text"
                    class="input input-bordered w-full !bg-base-100"
                    disabled
                />
            </label>

            <label class="form-control w-full max-w-md mt-3">
                <div class="label">
                    <span class="label-text">{{ t('user.mail') }}</span>
                </div>
                <input v-model="configStore.configUser.mail" type="email" class="input input-bordered w-full" />
            </label>

            <label class="form-control w-full max-w-md mt-3">
                <div class="label">
                    <span class="label-text">{{ t('user.newPass') }}</span>
                </div>
                <input v-model="newPass" type="password" class="input input-bordered w-full" />
            </label>

            <label class="form-control w-full max-w-md mt-3">
                <div class="label">
                    <span class="label-text">{{ t('user.confirmPass') }}</span>
                </div>
                <input v-model="confirmPass" type="password" class="input input-bordered w-full" />
            </label>

            <div v-if="authStore.role === 'global_admin'" class="form-control w-full max-w-md mt-5">
                <Multiselect
                    v-model="configStore.configUser.channel_ids"
                    :options="configStore.channels"
                    mode="tags"
                    :close-on-select="true"
                    :can-clear="false"
                    label="name"
                    value-prop="id"
                    :classes="multiSelectClasses"
                    :disabled="configStore.configUser.role_id === 1"
                />
            </div>

            <div>
                <button class="btn btn-primary mt-5" type="submit">{{ t('user.save') }}</button>
            </div>
        </form>
    </div>

    <GenericModal :show="showUserModal" title="Add user" :modal-action="addUser">
        <div class="w-full max-w-[500px] h-[490px]">
            <label class="form-control w-full">
                <div class="label">
                    <span class="label-text">{{ t('user.name') }}</span>
                </div>
                <input v-model="user.username" name="username" type="text" class="input input-bordered w-full" />
            </label>

            <label class="form-control w-full mt-3">
                <div class="label">
                    <span class="label-text">{{ t('user.mail') }}</span>
                </div>
                <input v-model="user.mail" name="mail" type="email" class="input input-bordered w-full" />
            </label>

            <label class="form-control w-full mt-3">
                <div class="label">
                    <span class="label-text">{{ t('user.password') }}</span>
                </div>
                <input v-model="user.password" type="password" class="input input-bordered w-full" />
            </label>

            <label class="form-control w-full mt-3">
                <div class="label">
                    <span class="label-text">{{ t('user.confirmPass') }}</span>
                </div>
                <input v-model="user.confirm" type="password" class="input input-bordered w-full" />
            </label>

            <div class="form-control mt-5">
                <Multiselect
                    v-model="user.channel_ids"
                    :options="configStore.channels"
                    mode="tags"
                    :close-on-select="true"
                    :can-clear="false"
                    label="name"
                    value-prop="id"
                    :classes="multiSelectClasses"
                />
            </div>

            <div class="form-control mt-3">
                <label class="label cursor-pointer w-1/2">
                    <span class="label-text">{{ t('user.admin') }}</span>
                    <input v-model.number="user.admin" type="checkbox" class="checkbox" />
                </label>
            </div>
        </div>
    </GenericModal>
</template>

<script setup lang="ts">
const { t } = useI18n()
const { multiSelectClasses } = useVariables()

const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()

const selected = ref(null as null | number)
const users = ref([] as User[])
const showUserModal = ref(false)
const newPass = ref('')
const confirmPass = ref('')

const user = ref({
    id: 0,
    username: '',
    mail: '',
    password: '',
    confirm: '',
    admin: false,
    channel_ids: [configStore.channels[configStore.i]?.id ?? 1],
    role_id: 3,
} as User)

onMounted(() => {
    if (authStore.role === 'global_admin') {
        getUsers()
    }
})

async function getUsers() {
    fetch('/api/users', {
        method: 'GET',
        headers: authStore.authHeader,
    })
        .then((response) => response.json())
        .then((data) => {
            users.value = data

            selected.value = configStore.currentUser
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
    if (configStore.configUser.id === configStore.currentUser) {
        indexStore.msgAlert('error', t('user.deleteNotPossible'), 2)
    } else {
        await fetch(`/api/user/${configStore.configUser.id}`, {
            method: 'DELETE',
            headers: authStore.authHeader,
        })
            .then(async () => {
                indexStore.msgAlert('success', t('user.deleteSuccess'), 2)

                await configStore.getUserConfig()
                await getUsers()
            })
            .catch((e) => {
                indexStore.msgAlert('error', `${t('user.deleteError')}: ${e}`, 2)
            })
    }
}

function clearUser() {
    user.value.id = 0
    user.value.username = ''
    user.value.mail = ''
    user.value.password = ''
    user.value.confirm = ''
    user.value.admin = false
    user.value.channel_ids = [1]
    user.value.role_id = 3
}

async function addUser(add: boolean) {
    if (add) {
        if (user.value.admin) {
            user.value.role_id = 2
        } else {
            user.value.role_id = 3
        }

        delete user.value.admin

        if (user.value.username && user.value.password && user.value.password === user.value.confirm) {
            await authStore.inspectToken()
            const update = await configStore.addNewUser(user.value)
            showUserModal.value = false

            if (update.status === 200) {
                indexStore.msgAlert('success', t('user.addSuccess'), 2)

                await getUsers()
                await getUserConfig()
            } else {
                indexStore.msgAlert('error', t('user.addFailed'), 3)
            }

            clearUser()
        } else {
            indexStore.msgAlert('error', t('user.mismatch'), 3)
        }
    } else {
        showUserModal.value = false
        clearUser()
    }
}

async function onSubmitUser() {
    if (newPass.value) {
        if (newPass.value === confirmPass.value) {
            configStore.configUser.password = newPass.value
        } else {
            indexStore.msgAlert('error', t('user.mismatch'), 3)
            return
        }
    }

    await authStore.inspectToken()
    const update = await configStore.setUserConfig(configStore.configUser)

    if (update.status === 200) {
        indexStore.msgAlert('success', t('user.updateSuccess'), 2)
    } else {
        indexStore.msgAlert('error', t('user.updateFailed'), 2)
    }

    newPass.value = ''
    confirmPass.value = ''
}
</script>
