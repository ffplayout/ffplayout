<template>
    <div>
        <div class="container">
            <h2 class="pb-4 pt-3">Channel Configuration</h2>
            <div style="width: 100%; height: 43px">
                <div class="float-end">
                    <button class="btn btn-primary" @click="addChannel()">Add new Channel</button>
                </div>
            </div>
            <form
                v-if="configStore.configGui && configStore.configGui[configStore.configID]"
                @submit.prevent="onSubmitGui"
            >
                <div class="mb-3 row">
                    <label for="configName" class="col-sm-2 col-form-label">Name</label>
                    <div class="col-sm-10">
                        <input
                            type="text"
                            class="form-control"
                            id="configName"
                            v-model="configStore.configGui[configStore.configID].name"
                        />
                    </div>
                </div>
                <div class="mb-3 row">
                    <label for="configUrl" class="col-sm-2 col-form-label">Preview URL</label>
                    <div class="col-sm-10">
                        <input
                            type="text"
                            class="form-control"
                            id="configUrl"
                            v-model="configStore.configGui[configStore.configID].preview_url"
                        />
                    </div>
                </div>
                <div class="mb-3 row">
                    <label for="configPath" class="col-sm-2 col-form-label">Config Path</label>
                    <div class="col-sm-10">
                        <input
                            type="text"
                            class="form-control"
                            id="configPath"
                            v-model="configStore.configGui[configStore.configID].config_path"
                            disabled
                        />
                    </div>
                </div>
                <div class="mb-3 row">
                    <label for="configExtensions" class="col-sm-2 col-form-label">Extra Extensions</label>
                    <div class="col-sm-10">
                        <input
                            type="text"
                            class="form-control"
                            id="configExtensions"
                            v-model="configStore.configGui[configStore.configID].extra_extensions"
                        />
                    </div>
                </div>
                <div class="mb-3 row">
                    <label for="configService" class="col-sm-2 col-form-label">Service</label>
                    <div class="col-sm-10">
                        <input
                            type="text"
                            class="form-control"
                            id="configService"
                            v-model="configStore.configGui[configStore.configID].service"
                            disabled
                        />
                    </div>
                </div>
                <div class="row">
                    <div class="col-1" style="min-width: 158px">
                        <div class="btn-group">
                            <button class="btn btn-primary" type="submit">Save</button>
                            <button
                                class="btn btn-danger"
                                v-if="
                                    configStore.configGui.length > 1 &&
                                    configStore.configGui[configStore.configID].id > 1
                                "
                                @click="deleteChannel()"
                            >
                                Delete
                            </button>
                        </div>
                    </div>
                </div>
            </form>
        </div>
    </div>
</template>

<script setup lang="ts">
import { useAuth } from '~/stores/auth'
import { useConfig } from '~/stores/config'
import { useIndex } from '~~/stores'

const { $_ } = useNuxtApp()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()

async function addChannel() {
    const channels = $_.cloneDeep(configStore.configGui)
    const newChannel = $_.cloneDeep(configStore.configGui[configStore.configGui.length - 1])

    const playoutConfigPath = newChannel.config_path.match(/.*\//)
    const confName = `channel${String(channels.length + 1).padStart(3, '0')}`

    newChannel.id = channels.length + 1
    newChannel.name = `Channel ${Math.random().toString(36).substring(7)}`
    newChannel.config_path = `${playoutConfigPath}${confName}.yml`
    newChannel.service = `ffplayout@${confName}.service`

    channels.push(newChannel)
    configStore.updateGuiConfig(channels)
    configStore.updateConfigID(configStore.configGui.length - 1)
}

async function onSubmitGui() {
    /*
        Save GUI settings.
    */
    const update = await configStore.setGuiConfig(configStore.configGui[configStore.configID])

    if (update.status) {
        indexStore.alertVariant = 'alert-success'
        indexStore.alertMsg = 'Update GUI config success!'
    } else {
        indexStore.alertVariant = 'alert-danger'
        indexStore.alertMsg = 'Update GUI config failed!'
    }

    indexStore.showAlert = true

    setTimeout(() => {
        indexStore.showAlert = false
    }, 2000)
}

async function deleteChannel() {
    const config = $_.cloneDeep(configStore.configGui)
    const id = config[configStore.configID].id

    if (id === 1) {
        indexStore.alertVariant = 'alert-warning'
        indexStore.alertMsg = 'First channel can not be deleted!'
        indexStore.showAlert = true
        return
    }

    const response = await fetch(`/api/channel/${id}`, {
        method: 'DELETE',
        headers: authStore.authHeader,
    })

    config.splice(configStore.configID, 1)
    configStore.updateGuiConfig(config)
    configStore.updateConfigID(configStore.configGui.length - 1)
    await configStore.getPlayoutConfig()

    if (response.status === 200) {
        indexStore.alertVariant = 'alert-success'
        indexStore.alertMsg = 'Delete GUI config success!'
    } else {
        indexStore.alertVariant = 'alert-danger'
        indexStore.alertMsg = 'Delete GUI config failed!'
    }

    indexStore.showAlert = true

    setTimeout(() => {
        indexStore.showAlert = false
    }, 2000)
}
</script>
