<template>
    <div class="flex flex-col items-center mt-10 px-8">
        <div class="mt-2 w-full max-w-4xl">
            <div class="flex max-w-md gap-4">
                <div>
                    <select
                        class="select select-sm select-bordered w-full max-w-md"
                        v-model="selected"
                        @change="onChange($event)"
                    >
                        <option v-for="item in presets">{{ item.name }}</option>
                    </select>
                </div>
                <div class="join">
                    <button class="btn btn-sm join-item btn-primary" title="Save Preset" @click="savePreset()">
                        <i class="bi-cloud-upload" />
                    </button>
                    <button class="btn btn-sm join-item btn-primary" title="New Preset" @click="showCreateModal = true">
                        <i class="bi-file-plus" />
                    </button>
                    <button
                        class="btn btn-sm join-item btn-primary"
                        title="Delete Preset"
                        @click="showDeleteModal = true"
                    >
                        <i class="bi-file-minus" />
                    </button>
                </div>
            </div>

            <form @submit.prevent="submitMessage" class="mt-6 w-full">
                <textarea
                    class="textarea textarea-bordered w-full"
                    v-model="form.text"
                    rows="4"
                    placeholder="Message"
                />

                <div class="mt-5 grid grid-cols-[auto_150px_150px] gap-4">
                    <div class="grow">
                        <input
                            class="input input-sm input-bordered w-full"
                            v-model="form.x"
                            type="text"
                            title="X Axis"
                            placeholder="X"
                            required
                        />
                        <input
                            class="input input-sm input-bordered w-full mt-6"
                            v-model="form.y"
                            type="text"
                            title="Y Axis"
                            data-tooltip="tooltip"
                            placeholder="Y"
                            required
                        />
                    </div>

                    <div>
                        <div class="form-control">
                            <label class="label cursor-pointer p-0">
                                <span class="label-text">Show Box</span>
                                <input type="checkbox" v-model="form.showBox" class="checkbox checkbox-xs rounded-sm" />
                            </label>
                        </div>

                        <label class="form-control w-full">
                            <div class="label">
                                <span class="label-text">Box Color</span>
                            </div>
                            <input
                                type="color"
                                class="input input-sm input-bordered w-full p-1"
                                v-model="form.boxColor"
                                required
                            />
                        </label>
                    </div>
                    <label class="form-control w-full mt-5">
                        <div class="label">
                            <span class="label-text">Box Alpha</span>
                        </div>
                        <input
                            type="number"
                            min="0"
                            max="1"
                            step="0.01"
                            class="input input-sm input-bordered w-full"
                            v-model="form.boxAlpha"
                            required
                        />
                    </label>
                </div>
                <div class="grid grid-cols-[150px_150px_auto] gap-4">
                    <div>
                        <label class="form-control w-full">
                            <div class="label">
                                <span class="label-text">Size</span>
                            </div>
                            <input
                                type="number"
                                class="input input-sm input-bordered w-full"
                                v-model="form.fontSize"
                                required
                            />
                        </label>

                        <label class="form-control w-full mt-2">
                            <div class="label">
                                <span class="label-text">Font Color</span>
                            </div>
                            <input
                                type="color"
                                class="input input-sm input-bordered w-full p-1"
                                v-model="form.fontColor"
                                required
                            />
                        </label>
                    </div>
                    <div>
                        <label class="form-control w-full">
                            <div class="label">
                                <span class="label-text">Spacing</span>
                            </div>
                            <input
                                type="number"
                                class="input input-sm input-bordered w-full"
                                v-model="form.fontSpacing"
                                required
                            />
                        </label>
                        <label class="form-control w-full mt-2">
                            <div class="label">
                                <span class="label-text">Font Alpha</span>
                            </div>
                            <input
                                type="number"
                                class="input input-sm input-bordered w-full"
                                v-model="form.fontAlpha"
                                min="0"
                                max="1"
                                step="0.01"
                                required
                            />
                        </label>
                    </div>

                    <div class="grow">
                        <label class="form-control w-full">
                            <div class="label">
                                <span class="label-text">Overall Alpha</span>
                            </div>
                            <input
                                type="text"
                                class="input input-sm input-bordered w-full"
                                v-model="form.overallAlpha"
                                required
                            />
                        </label>
                        <label class="form-control w-full max-w-[150px] mt-2">
                            <div class="label">
                                <span class="label-text">Border Width</span>
                            </div>
                            <input
                                type="number"
                                class="input input-sm input-bordered w-full"
                                v-model="form.border"
                                required
                            />
                        </label>
                    </div>
                </div>

                <div class="mt-5">
                    <button class="btn btn-primary send-btn" type="submit">Send</button>
                </div>
            </form>
        </div>
    </div>

    <div
        v-if="showCreateModal"
        class="z-50 fixed top-0 bottom-0 left-0 right-0 flex justify-center items-center bg-black/30"
    >
        <div class="flex flex-col bg-base-100 w-[400px] h-[200px] mt-[10%] rounded-md p-5 shadow-xl">
            <div class="font-bold text-lg">New Preset</div>

            <label class="form-control w-full">
                <div class="label">
                    <span class="label-text">Name</span>
                </div>
                <input type="text" class="input input-bordered w-full" v-model="newPresetName" />
            </label>

            <div class="mt-4 flex justify-end">
                <div class="join">
                    <button
                        class="btn btn-sm bg-base-300 hover:bg-base-300/50 join-item"
                        @click=";(newPresetName = ''), (showCreateModal = false)"
                    >
                        Cancel
                    </button>
                    <button class="btn btn-sm bg-base-300 hover:bg-base-300/50 join-item" @click="createNewPreset()">
                        Ok
                    </button>
                </div>
            </div>
        </div>
    </div>

    <Modal
        :show="showDeleteModal"
        title="Delete Preset"
        :text="`Are you sure that you want to delete preset: <strong> ${selected}</strong>?`"
        :modalAction="deletePreset"
    />
</template>

<script setup lang="ts">
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()
const { numberToHex, hexToNumber } = stringFormatter()
const contentType = { 'content-type': 'application/json;charset=UTF-8' }

useHead({
    title: 'Messages | ffplayout',
})

interface PresetName {
    name: string
    value: number
}

const form = ref({
    id: 0,
    name: '',
    text: '',
    x: '0',
    y: '0',
    fontSize: 24,
    fontSpacing: 4,
    fontColor: '#ffffff',
    fontAlpha: 1.0,
    showBox: true,
    boxColor: '#000000',
    boxAlpha: 0.8,
    border: 4,
    overallAlpha: '1',
})

const showCreateModal = ref(false)
const showDeleteModal = ref(false)
const selected = ref(null)
const newPresetName = ref('')
const presets = ref([] as PresetName[])

onMounted(() => {
    getPreset(-1)
})

async function getPreset(index: number) {
    fetch(`/api/presets/${configStore.configGui[configStore.configID].id}`, {
        method: 'GET',
        headers: authStore.authHeader,
    })
        .then((response) => response.json())
        .then((data) => {
            if (index === -1) {
                presets.value = [{ value: -1, name: '' }]

                for (let i = 0; i < data.length; i++) {
                    const elem = data[i]
                    presets.value.push({ value: i, name: elem.name })
                }

                form.value = {
                    id: 0,
                    name: '',
                    text: '',
                    x: '0',
                    y: '0',
                    fontSize: 24,
                    fontSpacing: 4,
                    fontColor: '#ffffff',
                    fontAlpha: 1.0,
                    showBox: true,
                    boxColor: '#000000',
                    boxAlpha: 0.8,
                    border: 4,
                    overallAlpha: '1',
                }
            } else {
                const fColor = data[index].fontcolor.split('@')
                const bColor = data[index].boxcolor.split('@')

                form.value = {
                    id: data[index].id,
                    name: data[index].name,
                    text: data[index].text,
                    x: data[index].x,
                    y: data[index].y,
                    fontSize: data[index].fontsize,
                    fontSpacing: data[index].line_spacing,
                    fontColor: fColor[0],
                    fontAlpha: fColor[1] ? hexToNumber(fColor[1]) : 1.0,
                    showBox: data[index].box === '1' ? true : false,
                    boxColor: bColor[0],
                    boxAlpha: bColor[1] ? hexToNumber(bColor[1]) : 1.0,
                    border: data[index].boxborderw,
                    overallAlpha: data[index].alpha,
                }
            }
        })
}

function onChange(event: any) {
    selected.value = event.target.value

    getPreset(event.target.selectedIndex - 1)
}

async function savePreset() {
    if (selected.value) {
        const preset = {
            id: form.value.id,
            name: form.value.name,
            text: form.value.text,
            x: form.value.x,
            y: form.value.y,
            fontsize: form.value.fontSize,
            line_spacing: form.value.fontSpacing,
            fontcolor:
                form.value.fontAlpha === 1
                    ? form.value.fontColor
                    : form.value.fontColor + '@' + numberToHex(form.value.fontAlpha),
            box: form.value.showBox ? '1' : '0',
            boxcolor:
                form.value.boxAlpha === 1
                    ? form.value.boxColor
                    : form.value.boxColor + '@' + numberToHex(form.value.boxAlpha),
            boxborderw: form.value.border,
            alpha: form.value.overallAlpha,
            channel_id: configStore.configGui[configStore.configID].id,
        }

        const response = await fetch(`/api/presets/${form.value.id}`, {
            method: 'PUT',
            headers: { ...contentType, ...authStore.authHeader },
            body: JSON.stringify(preset),
        })

        if (response.status === 200) {
            indexStore.msgAlert('alert-success', 'Save Preset done!', 2)
        } else {
            indexStore.msgAlert('alert-error', 'Save Preset failed!', 2)
        }
    }
}

async function createNewPreset() {
    showCreateModal.value = false

    const preset = {
        name: newPresetName.value,
        text: form.value.text,
        x: form.value.x.toString(),
        y: form.value.y.toString(),
        fontsize: form.value.fontSize.toString(),
        line_spacing: form.value.fontSpacing.toString(),
        fontcolor:
            form.value.fontAlpha === 1
                ? form.value.fontColor
                : form.value.fontColor + '@' + numberToHex(form.value.fontAlpha),
        box: form.value.showBox ? '1' : '0',
        boxcolor:
            form.value.boxAlpha === 1
                ? form.value.boxColor
                : form.value.boxColor + '@' + numberToHex(form.value.boxAlpha),
        boxborderw: form.value.border.toString(),
        alpha: form.value.overallAlpha.toString(),
        channel_id: configStore.configGui[configStore.configID].id,
    }

    const response = await fetch('/api/presets/', {
        method: 'POST',
        headers: { ...contentType, ...authStore.authHeader },
        body: JSON.stringify(preset),
    })

    if (response.status === 200) {
        indexStore.msgAlert('alert-success', 'Save Preset done!', 2)
        getPreset(-1)
    } else {
        indexStore.msgAlert('alert-error', 'Save Preset failed!', 2)
    }
}

async function deletePreset(del: boolean) {
    showDeleteModal.value = false

    if (del && selected.value && selected.value !== '') {
        await fetch(`/api/presets/${form.value.id}`, {
            method: 'DELETE',
            headers: authStore.authHeader,
        })

        getPreset(-1)
    }
}

async function submitMessage() {
    const obj = {
        text: form.value.text,
        x: form.value.x.toString(),
        y: form.value.y.toString(),
        fontsize: form.value.fontSize.toString(),
        line_spacing: form.value.fontSpacing.toString(),
        fontcolor: form.value.fontColor + '@' + numberToHex(form.value.fontAlpha),
        alpha: form.value.overallAlpha.toString(),
        box: form.value.showBox ? '1' : '0',
        boxcolor: form.value.boxColor + '@' + numberToHex(form.value.boxAlpha),
        boxborderw: form.value.border.toString(),
    }

    const response = await fetch(`/api/control/${configStore.configGui[configStore.configID].id}/text/`, {
        method: 'POST',
        headers: { ...contentType, ...authStore.authHeader },
        body: JSON.stringify(obj),
    })

    if (response.status === 200) {
        indexStore.msgAlert('alert-success', 'Sending success...', 2)
    } else {
        indexStore.msgAlert('alert-error', 'Sending failed...', 2)
    }
}
</script>
