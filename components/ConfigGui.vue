<template>
    <div class="w-full max-w-[800px]">
        <h2 class="pt-3 text-3xl">Channel Configuration</h2>
        <div class="w-full flex justify-end my-4">
            <button class="btn btn-sm btn-primary" @click="addChannel()">Add new Channel</button>
        </div>
        <form
            v-if="configStore.configGui && configStore.configGui[configStore.configID]"
            @submit.prevent="onSubmitGui"
            class="w-full"
        >
            <label class="form-control w-full">
                <div class="label">
                    <span class="label-text">Name</span>
                </div>
                <input
                    type="text"
                    placeholder="Type here"
                    class="input input-bordered w-full"
                    v-model="configStore.configGui[configStore.configID].name"
                />
            </label>

            <label class="form-control w-full mt-5">
                <div class="label">
                    <span class="label-text">Preview URL</span>
                </div>
                <input
                    type="text"
                    placeholder="Type here"
                    class="input input-bordered w-full"
                    v-model="configStore.configGui[configStore.configID].preview_url"
                />
            </label>

            <label class="form-control w-full mt-5">
                <div class="label">
                    <span class="label-text">Config Path</span>
                </div>
                <input
                    type="text"
                    placeholder="Type here"
                    class="input input-bordered w-full"
                    v-model="configStore.configGui[configStore.configID].config_path"
                />
            </label>

            <label class="form-control w-full mt-5">
                <div class="label">
                    <span class="label-text">Extra Extensions</span>
                </div>
                <input
                    type="text"
                    placeholder="Type here"
                    class="input input-bordered w-full"
                    v-model="configStore.configGui[configStore.configID].extra_extensions"
                />
            </label>

            <label class="form-control w-full mt-5">
                <div class="label">
                    <span class="label-text">Service</span>
                </div>
                <input
                    type="text"
                    placeholder="Type here"
                    class="input input-bordered w-full"
                    v-model="configStore.configGui[configStore.configID].service"
                    disabled
                />
            </label>

            <div class="join mt-4">
                <button class="join-item btn btn-primary" type="submit">Save</button>
                <button
                    class="join-item btn btn-primary"
                    v-if="configStore.configGui.length > 1 && configStore.configGui[configStore.configID].id > 1"
                    @click="deleteChannel()"
                >
                    Delete
                </button>
            </div>
        </form>
    </div>
</template>

<script setup lang="ts">
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
    configStore.configGui = channels
    configStore.configID = configStore.configGui.length - 1
}

async function onSubmitGui() {
    /*
        Save GUI settings.
    */
    const update = await configStore.setGuiConfig(configStore.configGui[configStore.configID])

    if (update.status) {
        indexStore.msgAlert('alert-success', 'Update GUI config success!', 2)
    } else {
        indexStore.msgAlert('alert-error', 'Update GUI config failed!', 2)
    }
}

async function deleteChannel() {
    const config = $_.cloneDeep(configStore.configGui)
    const id = config[configStore.configID].id

    if (id === 1) {
        indexStore.msgAlert('alert-warning', 'First channel can not be deleted!', 2)
        return
    }

    const response = await fetch(`/api/channel/${id}`, {
        method: 'DELETE',
        headers: authStore.authHeader,
    })

    config.splice(configStore.configID, 1)
    configStore.configGui = config
    configStore.configID = configStore.configGui.length - 1
    await configStore.getPlayoutConfig()

    if (response.status === 200) {
        indexStore.msgAlert('alert-success', 'Delete GUI config success!', 2)
    } else {
        indexStore.msgAlert('alert-error', 'Delete GUI config failed!', 2)
    }
}
</script>
