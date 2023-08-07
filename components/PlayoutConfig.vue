<template>
    <div>
        <div class="container">
            <h2 class="pb-4 pt-3">Playout Configuration</h2>
            <form v-if="configStore.configPlayout" @submit.prevent="onSubmitPlayout">
                <div v-for="(item, key) in configStore.configPlayout" class="mb-2 row" :key="key">
                    <div class="col-sm-1">
                        <strong>{{ key }}:</strong>
                    </div>
                    <div class="col-sm-11 pb-4 mt-4">
                        <div v-for="(prop, name) in (item as Record<string, any>)" class="mb-2 row">
                            <label :for="name" class="col-sm-2 col-form-label">{{ name }}:</label>
                            <div class="col-sm-10">
                                <div v-if="name.toString() === 'help_text'" class="pt-2 pb-2">{{ prop }}</div>
                                <input
                                    v-else-if="name.toString() === 'sender_pass'"
                                    type="password"
                                    class="form-control"
                                    :id="name"
                                    v-model="item[name]"
                                />
                                <textarea
                                    v-else-if="
                                        name.toString() === 'output_param' || name.toString() === 'custom_filter'
                                    "
                                    class="form-control"
                                    :id="name"
                                    v-model="item[name]"
                                    rows="5"
                                />
                                <input
                                    v-else-if="typeof prop === 'number' && prop % 1 === 0"
                                    type="number"
                                    class="form-control"
                                    :id="name"
                                    v-model="item[name]"
                                    style="max-width: 250px"
                                />
                                <input
                                    v-else-if="typeof prop === 'number'"
                                    type="number"
                                    class="form-control"
                                    :id="name"
                                    v-model="item[name]"
                                    step="0.0001"
                                    style="max-width: 250px"
                                />
                                <input
                                    v-else-if="typeof prop === 'boolean'"
                                    type="checkbox"
                                    class="form-check-input mt-2"
                                    :id="name"
                                    v-model="item[name]"
                                />
                                <input v-else type="text" class="form-control" :id="name" v-model="item[name]" />
                            </div>
                        </div>
                    </div>
                </div>
                <div class="row">
                    <div class="col-1" style="min-width: 85px">
                        <button class="btn btn-primary" type="submit" variant="primary">Save</button>
                    </div>
                </div>
            </form>
        </div>

        <div
            id="restartModal"
            ref="restartModal"
            class="modal"
            tabindex="-1"
            aria-labelledby="restartModalLabel"
            aria-hidden="true"
        >
            <div class="modal-dialog modal-dialog-centered">
                <div class="modal-content">
                    <div class="modal-header">
                        <h1 class="modal-title fs-5" id="restartModalLabel">Restart Playout</h1>
                        <button type="button" class="btn-close" data-bs-dismiss="modal" aria-label="Cancel"></button>
                    </div>
                    <div class="modal-body">Restart ffplayout to apply changes?</div>
                    <div class="modal-footer">
                        <button type="button" class="btn btn-primary" data-bs-dismiss="modal">No</button>
                        <button type="button" class="btn btn-primary" data-bs-dismiss="modal" @click="restart()">
                            Yes
                        </button>
                    </div>
                </div>
            </div>
        </div>
    </div>
</template>

<script setup lang="ts">
import { useAuth } from '~/stores/auth'
import { useConfig } from '~/stores/config'
import { useIndex } from '~/stores/index'

const { $bootstrap } = useNuxtApp()

const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()

const contentType = { 'content-type': 'application/json;charset=UTF-8' }

const restartModal = ref()

async function onSubmitPlayout() {
    const update = await configStore.setPlayoutConfig(configStore.configPlayout)

    if (update.status === 200) {
        indexStore.alertVariant = 'alert-success'
        indexStore.alertMsg = 'Update playout config success!'

        const channel = configStore.configGui[configStore.configID].id

        await $fetch(`/api/control/${channel}/process/`, {
            method: 'POST',
            headers: { ...contentType, ...authStore.authHeader },
            body: JSON.stringify({ command: 'status' }),
        }).then((response: any) => {
            if (response === 'active') {
                console.log('---restart modal')
                // @ts-ignore
                const modal = $bootstrap.Modal.getOrCreateInstance(restartModal.value)

                modal.show()
            }
        })
    } else {
        indexStore.alertVariant = 'alert-danger'
        indexStore.alertMsg = 'Update playout config failed!'
    }

    indexStore.showAlert = true

    setTimeout(() => {
        indexStore.showAlert = false
    }, 2000)
}

async function restart() {
    const channel = configStore.configGui[configStore.configID].id

    await $fetch(`/api/control/${channel}/process/`, {
        method: 'POST',
        headers: { ...contentType, ...authStore.authHeader },
        body: JSON.stringify({ command: 'restart' }),
    })
}
</script>
