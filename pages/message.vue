<template>
    <div>
        <Menu />
        <div class="container mt-5">
            <div class="preset-div">
                <div class="row">
                    <div class="col">
                        <select class="form-select" v-model="selected" @change="onChange($event)">
                            <option v-for="item in presets">{{ item.name }}</option>
                        </select>
                    </div>
                    <div class="col-2">
                        <div class="btn-group" role="group">
                            <button class="btn btn-primary" title="Save Preset" data-tooltip=tooltip @click="savePreset()">
                                <i class="bi-cloud-upload" />
                            </button>
                            <button
                                class="btn btn-primary"
                                title="New Preset"
                                data-tooltip=tooltip
                                data-bs-toggle="modal"
                                data-bs-target="#createModal"
                            >
                                <i class="bi-file-plus" />
                            </button>
                            <button
                                class="btn btn-primary"
                                title="Delete Preset"
                                data-tooltip=tooltip
                                data-bs-toggle="modal"
                                data-bs-target="#deleteModal"
                            >
                                <i class="bi-file-minus" />
                            </button>
                        </div>
                    </div>
                </div>
            </div>

            <form @submit.prevent="submitMessage">
                <textarea class="form-control message" v-model="form.text" rows="7" placeholder="Message" />

                <div class="row mt-3">
                    <div class="col">
                        <input
                            class="form-control mt-1"
                            v-model="form.x"
                            type="text"
                            title="X Axis"
                            data-tooltip=tooltip
                            placeholder="X"
                            required
                        />
                        <input
                            class="form-control mt-2"
                            v-model="form.y"
                            type="text"
                            title="Y Axis"
                            data-tooltip=tooltip
                            placeholder="Y"
                            required
                        />

                        <div class="row mt-2">
                            <div class="col">
                                <label for="input-size">Size</label>
                                <input
                                    id="input-size"
                                    class="form-control mt-2"
                                    v-model="form.fontSize"
                                    type="number"
                                    required
                                />
                            </div>
                            <div class="col">
                                <label for="input-spacing">Spacing</label>
                                <input
                                    id="input-spacing"
                                    class="form-control mt-2"
                                    v-model="form.fontSpacing"
                                    type="number"
                                    required
                                />
                            </div>
                        </div>

                        <div class="row mt-2">
                            <div class="col">
                                <label for="input-color">Font Color</label>
                                <input
                                    id="input-color"
                                    class="form-control mt-2"
                                    v-model="form.fontColor"
                                    type="color"
                                    required
                                />
                            </div>
                            <div class="col">
                                <label for="input-alpha">Font Alpha</label>
                                <input
                                    id="input-alpha"
                                    class="form-control mt-2"
                                    v-model="form.fontAlpha"
                                    type="number"
                                    min="0"
                                    max="1"
                                    step="0.01"
                                />
                            </div>
                        </div>
                    </div>

                    <div class="col">
                        <div class="form-check">
                            <input id="input-box" type="checkbox" class="form-check-input" v-model="form.showBox" />
                            <label for="input-box" class="form-check-label">Show Box</label>
                        </div>

                        <div class="row">
                            <div class="col">
                                <label for="input-box-color">Box Color</label>
                                <input
                                    id="input-box-color"
                                    class="form-control mt-2"
                                    v-model="form.boxColor"
                                    type="color"
                                    required
                                />
                            </div>
                            <div class="col">
                                <label for="input-box-alpha" class="form-check-label">Box Alpha</label>
                                <input
                                    id="input-box-alpha"
                                    class="form-control mt-2"
                                    v-model="form.boxAlpha"
                                    type="number"
                                    min="0"
                                    max="1"
                                    step="0.01"
                                />
                            </div>
                            <label for="input-border-w" class="form-check-label">Border Width</label>
                            <input
                                id="input-border-w"
                                class="form-control mt-2"
                                v-model="form.border"
                                type="number"
                                required
                            />
                            <label for="input-overall-alpha" class="form-check-label mt-2">Overall Alpha</label>
                            <input
                                id="input-overall-alpha"
                                class="form-control mt-2"
                                v-model="form.overallAlpha"
                                type="text"
                                required
                            />
                        </div>
                    </div>
                </div>

                <div class="row mt-4">
                    <div class="col sub-btn">
                        <button class="btn btn-primary send-btn" type="submit">Send</button>
                    </div>
                </div>
            </form>
        </div>

        <div class="modal fade" id="createModal" tabindex="-1" aria-labelledby="createModalLabel" aria-hidden="true">
            <div class="modal-dialog modal-dialog-centered">
                <div class="modal-content">
                    <div class="modal-header">
                        <h1 class="modal-title fs-5" id="createModalLabel">New Preset</h1>
                        <button type="button" class="btn-close" data-bs-dismiss="modal" aria-label="Cancel"></button>
                    </div>
                    <div class="modal-body">
                        <form>
                            <div class="mb-3">
                                <label for="preset-name" class="col-form-label">Name:</label>
                                <input type="text" class="form-control" id="preset-name" v-model="newPresetName" />
                            </div>
                        </form>
                    </div>
                    <div class="modal-footer">
                        <button type="button" class="btn btn-primary" data-bs-dismiss="modal">Cancel</button>
                        <button
                            type="button"
                            class="btn btn-primary"
                            @click="createNewPreset()"
                            data-bs-dismiss="modal"
                        >
                            Save
                        </button>
                    </div>
                </div>
            </div>
        </div>

        <div class="modal fade" id="deleteModal" tabindex="-1" aria-labelledby="deleteModalLabel" aria-hidden="true">
            <div class="modal-dialog modal-dialog-centered">
                <div class="modal-content">
                    <div class="modal-header">
                        <h1 class="modal-title fs-5" id="deleteModalLabel">Delete Preset</h1>
                        <button type="button" class="btn-close" data-bs-dismiss="modal" aria-label="Cancel"></button>
                    </div>
                    <div class="modal-body">Are you sure that you want to delete preset: "{{ selected }}"?</div>
                    <div class="modal-footer">
                        <button type="button" class="btn btn-primary" data-bs-dismiss="modal">Cancel</button>
                        <button type="button" class="btn btn-primary" @click="deletePreset()" data-bs-dismiss="modal">
                            Ok
                        </button>
                    </div>
                </div>
            </div>
        </div>
    </div>
</template>

<script setup lang="ts">
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()
const { numberToHex, hexToNumber } = stringFormatter()
const contentType = { 'content-type': 'application/json;charset=UTF-8' }

useHead({
    title: 'Messages | ffplayout'
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

async function deletePreset() {
    if (selected.value && selected.value !== '') {
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

<style scoped>
.preset-div {
    width: 50%;
    margin-bottom: 2em;
}

.sub-btn {
    min-width: 90px;
    max-width: 100px;
}
</style>
