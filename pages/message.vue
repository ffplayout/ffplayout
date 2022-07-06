<template>
    <div>
        <Menu />
        <b-container class="messege-container">
            <div class="preset-div">
                <b-row>
                    <b-col>
                        <b-form-select v-model="selected" :options="presets" />
                    </b-col>
                    <b-col cols="2">
                        <b-button-group class="mr-1">
                            <b-button title="Save Preset" variant="primary" @click="savePreset()">
                                <b-icon icon="cloud-upload" />
                            </b-button>
                            <b-button title="New Preset" variant="primary" @click="openDialog()">
                                <b-icon-file-plus />
                            </b-button>
                            <b-button title="Delete Preset" variant="primary" @click="deleteDialog()">
                                <b-icon-file-minus />
                            </b-button>
                        </b-button-group>
                    </b-col>
                </b-row>
            </div>
            <b-form @submit.prevent="submitMessage">
                <b-form-group>
                    <b-form-textarea
                        v-model="form.text"
                        placeholder="Message"
                        rows="7"
                        class="message"
                    />
                </b-form-group>

                <b-row>
                    <b-col>
                        <b-form-group>
                            <b-form-input
                                id="input-1"
                                v-model="form.x"
                                type="text"
                                required
                                placeholder="X"
                            />
                        </b-form-group>

                        <b-form-group>
                            <b-form-input
                                id="input-2"
                                v-model="form.y"
                                type="text"
                                required
                                placeholder="Y"
                            />
                        </b-form-group>

                        <b-row>
                            <b-col>
                                <b-form-group
                                    label="Size"
                                    label-for="input-3"
                                >
                                    <b-form-input
                                        id="input-3"
                                        v-model="form.fontSize"
                                        type="number"
                                        required
                                        value="24"
                                    />
                                </b-form-group>
                            </b-col>
                            <b-col>
                                <b-form-group
                                    label="Spacing"
                                    label-for="input-4"
                                >
                                    <b-form-input
                                        id="input-4"
                                        v-model="form.fontSpacing"
                                        type="number"
                                        required
                                        value="4"
                                    />
                                </b-form-group>
                            </b-col>
                        </b-row>

                        <b-row>
                            <b-col>
                                <b-form-group
                                    label="Font Color"
                                    label-for="input-5"
                                >
                                    <b-form-input
                                        id="input-5"
                                        v-model="form.fontColor"
                                        type="color"
                                        required
                                    />
                                </b-form-group>
                            </b-col>
                            <b-col>
                                <b-form-group
                                    label="Font Alpha"
                                    label-for="input-6"
                                >
                                    <b-form-input
                                        id="input-6"
                                        v-model="form.fontAlpha"
                                        type="number"
                                        min="0"
                                        max="1"
                                        step="0.01"
                                    />
                                </b-form-group>
                            </b-col>
                        </b-row>
                    </b-col>
                    <b-col>
                        <b-form-checkbox
                            v-model="form.showBox"
                            style="margin-bottom: 8px;"
                        >
                            Show Box
                        </b-form-checkbox>

                        <b-row>
                            <b-col>
                                <b-form-group
                                    label="Box Color"
                                    label-for="input-7"
                                >
                                    <b-form-input
                                        id="input-7"
                                        v-model="form.boxColor"
                                        type="color"
                                        required
                                    />
                                </b-form-group>
                            </b-col>
                            <b-col>
                                <b-form-group
                                    label="Box Alpha"
                                    label-for="input-8"
                                >
                                    <b-form-input
                                        id="input-8"
                                        v-model="form.boxAlpha"
                                        type="number"
                                        min="0"
                                        max="1"
                                        step="0.01"
                                    />
                                </b-form-group>
                            </b-col>
                        </b-row>
                        <b-form-group
                            label="Border Width"
                            label-for="input-9"
                        >
                            <b-form-input
                                id="input-9"
                                v-model="form.border"
                                type="number"
                                required
                                value="4"
                            />
                        </b-form-group>
                    </b-col>
                </b-row>

                <b-form-group
                    label="Overall Alpha"
                    label-for="input-10"
                >
                    <b-form-input
                        id="input-10"
                        v-model="form.overallAlpha"
                        type="text"
                        required
                        value="1"
                    />
                </b-form-group>

                <b-row>
                    <b-col class="sub-btn">
                        <b-button type="submit" class="send-btn" variant="primary">
                            Send
                        </b-button>
                    </b-col>
                    <b-col>
                        <b-alert variant="success" :show="success" dismissible @dismissed="success=false">
                            Sending success...
                        </b-alert>
                        <b-alert variant="warning" :show="failed" dismissible @dismissed="success=failed">
                            Sending failed...
                        </b-alert>
                    </b-col>
                </b-row>
            </b-form>
        </b-container>
        <b-modal
            id="create-modal"
            ref="create-modal"
            title="Create Preset"
            @ok="handleCreate"
        >
            <form ref="form" @submit.stop.prevent="createPreset">
                <b-form-group label="Name" label-for="name-input" invalid-feedback="Name is required">
                    <b-form-input id="name-input" v-model="newPresetName" required />
                </b-form-group>
            </form>
        </b-modal>
        <b-modal
            id="delete-modal"
            ref="delete-modal"
            title="Delete Preset"
            @ok="handleDelete"
        >
            <strong>Delete: "{{ selected }}"?</strong>
        </b-modal>
    </div>
</template>

<script>
import { mapState } from 'vuex'
import Menu from '@/components/Menu.vue'

export default {
    name: 'Media',

    components: {
        Menu
    },

    middleware: 'auth',

    data () {
        return {
            form: {
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
                overallAlpha: 1
            },
            selected: null,
            newPresetName: '',
            presets: [],
            success: false,
            failed: false
        }
    },

    computed: {
        ...mapState('config', ['configID', 'configGui'])
    },

    watch: {
        selected (index) {
            this.getPreset(index)
        }
    },

    created () {
        this.getPreset(null)
    },

    methods: {
        decToHex (num) {
            return '0x' + Math.round(num * 255).toString(16)
        },

        hexToDec (num) {
            return (parseFloat(parseInt(num, 16)) / 255).toFixed(2)
        },

        async getPreset (index) {
            const response = await this.$axios.get(`api/presets/${this.configGui[this.configID].id}`)

            if (response.data && index === null) {
                this.presets = []
                for (let index = 0; index < response.data.length; index++) {
                    const elem = response.data[index]
                    this.presets.push({ value: index, text: elem.name })
                }
            } else if (response.data) {
                const fColor = response.data[index].fontcolor.split('@')
                const bColor = response.data[index].boxcolor.split('@')

                this.form = {
                    id: response.data[index].id,
                    name: response.data[index].name,
                    text: response.data[index].text,
                    x: response.data[index].x,
                    y: response.data[index].y,
                    fontSize: response.data[index].fontsize,
                    fontSpacing: response.data[index].line_spacing,
                    fontColor: fColor[0],
                    fontAlpha: (fColor[1]) ? this.hexToDec(fColor[1]) : 1.0,
                    showBox: response.data[index].box,
                    boxColor: bColor[0],
                    boxAlpha: (bColor[1]) ? this.hexToDec(bColor[1]) : 1.0,
                    border: response.data[index].boxborderw,
                    overallAlpha: response.data[index].alpha
                }
            }
        },
        openDialog () {
            this.$bvModal.show('create-modal')
        },
        handleCreate (bvModalEvt) {
            // Prevent modal from closing
            bvModalEvt.preventDefault()
            // Trigger submit handler
            this.createPreset()
        },
        async createPreset () {
            const preset = {
                name: this.newPresetName,
                text: this.form.text,
                x: this.form.x.toString(),
                y: this.form.y.toString(),
                fontsize: this.form.fontSize.toString(),
                line_spacing: this.form.fontSpacing.toString(),
                fontcolor: (this.form.fontAlpha === 1) ? this.form.fontColor : this.form.fontColor + '@' + this.decToHex(this.form.fontAlpha),
                box: (this.form.showBox) ? '1' : '0',
                boxcolor: (this.form.boxAlpha === 1) ? this.form.boxColor : this.form.boxColor + '@' + this.decToHex(this.form.boxAlpha),
                boxborderw: this.form.border.toString(),
                alpha: this.form.overallAlpha.toString(),
                channel_id: this.configGui[this.configID].id
            }

            const response = await this.$axios.post('api/presets/', preset)

            if (response.status === 200) {
                this.success = true
                this.getPreset(null)
            } else {
                this.failed = true
            }

            this.$nextTick(() => {
                this.$bvModal.hide('create-modal')
            })
        },
        async savePreset () {
            if (this.selected) {
                const preset = {
                    id: this.form.id,
                    name: this.form.name,
                    text: this.form.text,
                    x: this.form.x,
                    y: this.form.y,
                    fontsize: this.form.fontSize,
                    line_spacing: this.form.fontSpacing,
                    fontcolor: (this.form.fontAlpha === 1) ? this.form.fontColor : this.form.fontColor + '@' + this.decToHex(this.form.fontAlpha),
                    box: (this.form.showBox) ? '1' : '0',
                    boxcolor: (this.form.boxAlpha === 1) ? this.form.boxColor : this.form.boxColor + '@' + this.decToHex(this.form.boxAlpha),
                    boxborderw: this.form.border,
                    alpha: this.form.overallAlpha,
                    channel_id: this.configGui[this.configID].id
                }

                const response = await this.$axios.put(`api/presets/${this.form.id}`, preset)

                if (response.status === 200) {
                    this.success = true
                } else {
                    this.failed = true
                }
            }
        },

        deleteDialog () {
            this.$bvModal.show('delete-modal')
        },
        handleDelete (evt) {
            evt.preventDefault()
            this.deletePreset()
        },
        async deletePreset () {
            if (this.selected) {
                await this.$axios.delete(`api/presets/${this.form.id}`)
            }

            this.$bvModal.hide('delete-modal')
            this.getPreset(null)
        },

        async submitMessage () {
            const obj = {
                text: this.form.text,
                x: this.form.x.toString(),
                y: this.form.y.toString(),
                fontsize: this.form.fontSize.toString(),
                line_spacing: this.form.fontSpacing.toString(),
                fontcolor: this.form.fontColor + '@' + this.decToHex(this.form.fontAlpha),
                alpha: this.form.overallAlpha.toString(),
                box: (this.form.showBox) ? '1' : '0',
                boxcolor: this.form.boxColor + '@' + this.decToHex(this.form.boxAlpha),
                boxborderw: this.form.border.toString()
            }

            const response = await this.$axios.post(`api/control/${this.configGui[this.configID].id}/text/`, obj)

            if (response.data && response.status === 200) {
                this.success = true
            } else {
                this.failed = true
            }
        }
    }
}
</script>

<style scoped>
.messege-container {
    margin-top: 5em;
}

.preset-div {
    width: 50%;
    margin-bottom: 2em;
}

.sub-btn {
    min-width: 90px;
    max-width: 100px;
}
</style>
