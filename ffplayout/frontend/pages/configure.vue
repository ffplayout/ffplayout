<template>
    <div>
        <Menu />
        <b-card no-body>
            <b-tabs pills card vertical>
                <b-tab title="GUI" active @click="resetAlert()">
                    <b-container class="config-container">
                        <b-form v-if="configGui" @submit="onSubmitGui">
                            <b-form-group
                                label-cols-lg="2"
                                label="GUI Configuration"
                                label-size="lg"
                                label-class="font-weight-bold pt-0"
                                class="config-group"
                            >
                                <b-form-group
                                    v-for="(prop, name, idx) in configGui"
                                    :key="idx"
                                    label-cols-sm="2"
                                    :label="name"
                                    label-align-sm="right"
                                    :label-for="name"
                                >
                                    <b-form-tags
                                        v-if="name === 'extra_extensions'"
                                        v-model="configGui[name]"
                                        :input-id="name"
                                        separator=" ,;"
                                        :placeholder="`add ${name}...`"
                                        class="mb-2 tags-list"
                                    />
                                    <b-form-text v-if="name === 'extra_extensions'">
                                        Visible extensions only for the GUI and not the playout
                                    </b-form-text>
                                    <b-form-select v-else-if="name === 'net_interface'" :id="name" v-model="configGui[name]" :options="netChoices" :value="prop" />
                                    <b-form-input v-else :id="name" v-model="configGui[name]" :value="prop" />
                                </b-form-group>
                            </b-form-group>
                            <b-row>
                                <b-col cols="1" style="min-width: 85px">
                                    <b-button type="submit" variant="primary">
                                        Save
                                    </b-button>
                                </b-col>
                                <b-col>
                                    <b-alert v-model="showAlert" :variant="alertVariant" dismissible>
                                        {{ alertMsg }}
                                    </b-alert>
                                </b-col>
                            </b-row>
                        </b-form>
                    </b-container>
                </b-tab>
                <b-tab title="Playout" @click="resetAlert()">
                    <b-container class="config-container">
                        <b-form v-if="configPlayout" @submit="onSubmitPlayout">
                            <b-form-group
                                v-for="(item, key, index) in configPlayout"
                                :key="index"
                                label-cols-lg="2"
                                :label="key"
                                label-size="lg"
                                label-class="font-weight-bold pt-0"
                                class="config-group"
                            >
                                <b-form-group
                                    v-for="(prop, name, idx) in item"
                                    :key="idx"
                                    label-cols-sm="2"
                                    :label="(typeof prop === 'boolean' || name === 'helptext') ? '' : name"
                                    label-align-sm="right"
                                    :label-for="name"
                                >
                                    <b-form-textarea
                                        v-if="name === 'helptext'"
                                        id="textarea-plaintext"
                                        plaintext
                                        :value="prop"
                                        rows="2"
                                        max-rows="8"
                                        class="text-area"
                                    />
                                    <b-form-checkbox
                                        v-else-if="typeof prop === 'boolean'"
                                        :id="name"
                                        v-model="configPlayout[key][name]"
                                        :name="name"
                                    >
                                        {{ name }}
                                    </b-form-checkbox>
                                    <b-form-input
                                        v-else-if="prop && prop.toString().match(/^-?\d+[.,]\d+$/)"
                                        :id="name"
                                        v-model="configPlayout[key][name]"
                                        type="number"
                                        step="0.001"
                                        class="input-field"
                                    />
                                    <b-form-input
                                        v-else-if="prop && !isNaN(prop)"
                                        :id="name"
                                        v-model="configPlayout[key][name]"
                                        type="number"
                                        step="1"
                                        class="input-field"
                                    />
                                    <b-form-tags
                                        v-else-if="Array.isArray(prop)"
                                        v-model="configPlayout[key][name]"
                                        :input-id="name"
                                        separator=" ,;"
                                        :placeholder="`add ${name}...`"
                                        class="mb-2 tags-list"
                                    />
                                    <b-form-input
                                        v-else-if="name.includes('pass')"
                                        :id="name"
                                        v-model="configPlayout[key][name]"
                                        type="password"
                                        :value="prop"
                                    />
                                    <b-form-input v-else :id="name" v-model="configPlayout[key][name]" :value="prop" />
                                </b-form-group>
                            </b-form-group>

                            <b-row>
                                <b-col cols="1" style="min-width: 85px">
                                    <b-button type="submit" variant="primary">
                                        Save
                                    </b-button>
                                </b-col>
                                <b-col>
                                    <b-alert v-model="showAlert" :variant="alertVariant" dismissible>
                                        {{ alertMsg }}
                                    </b-alert>
                                </b-col>
                            </b-row>
                        </b-form>
                    </b-container>
                </b-tab>
                <b-tab title="User" @click="resetAlert()">
                    <b-card-text>
                        <b-container class="config-container">
                            <b-form v-if="configUser" @submit="onSubmitUser">
                                <b-form-group
                                    label-cols-lg="2"
                                    label="User Configuration"
                                    label-size="lg"
                                    label-class="font-weight-bold pt-0"
                                    class="config-group"
                                >
                                    <b-form-group
                                        label-cols-sm="2"
                                        :label="'username'"
                                        label-align-sm="right"
                                        :label-for="'username'"
                                    >
                                        <b-form-input id="username" v-model="configUser['username']" :value="configUser['username']" disabled />
                                    </b-form-group>
                                    <b-form-group
                                        label-cols-sm="2"
                                        :label="'email'"
                                        label-align-sm="right"
                                        :label-for="'email'"
                                    >
                                        <b-form-input id="email" v-model="configUser['email']" :value="configUser['email']" />
                                    </b-form-group>
                                    <b-form-group
                                        label-cols-sm="2"
                                        label="old password"
                                        label-align-sm="right"
                                        label-for="oldPass"
                                    >
                                        <b-form-input id="oldPass" v-model="oldPass" type="password" />
                                    </b-form-group>
                                    <b-form-group
                                        label-cols-sm="2"
                                        label="new password"
                                        label-align-sm="right"
                                        label-for="newPass"
                                    >
                                        <b-form-input id="newPass" v-model="newPass" type="password" />
                                    </b-form-group>
                                    <b-form-group
                                        label-cols-sm="2"
                                        label="confirm password"
                                        label-align-sm="right"
                                        label-for="confirmPass"
                                    >
                                        <b-form-input id="confirmPass" v-model="confirmPass" type="password" />
                                    </b-form-group>
                                </b-form-group>
                                <b-row>
                                    <b-col cols="1" style="min-width: 85px">
                                        <b-button type="submit" variant="primary">
                                            Save
                                        </b-button>
                                    </b-col>
                                    <b-col>
                                        <b-alert v-model="showAlert" :variant="alertVariant" dismissible>
                                            {{ alertMsg }}
                                        </b-alert>
                                    </b-col>
                                </b-row>
                            </b-form>
                        </b-container>
                    </b-card-text>
                </b-tab>
            </b-tabs>
        </b-card>
        <Login :show="showLogin" />
    </div>
</template>

<script>
import Menu from '@/components/Menu.vue'
import Login from '@/components/Login.vue'

export default {
    name: 'Configure',

    components: {
        Menu,
        Login
    },

    async asyncData ({ app, store }) {
        await store.dispatch('auth/inspectToken')
        let login = false

        if (store.state.auth.isLogin) {
            await store.dispatch('config/getGuiConfig')
            await store.dispatch('config/getPlayoutConfig')
            await store.dispatch('config/getUserConfig')
        } else {
            login = true
        }

        return {
            configGui: store.state.config.configGui,
            netChoices: store.state.config.netChoices,
            configPlayout: store.state.config.configPlayout,
            configUser: store.state.config.configUser,
            oldPass: null,
            newPass: null,
            confirmPass: null,
            showLogin: login,
            showAlert: false,
            alertVariant: 'success',
            alertMsg: ''
        }
    },

    data () {
        return {
        }
    },

    methods: {
        async onSubmitGui (evt) {
            evt.preventDefault()
            await this.$store.dispatch('auth/inspectToken')
            const update = await this.$store.dispatch('config/setGuiConfig', this.configGui)

            if (update.status === 200) {
                this.alertVariant = 'success'
                this.alertMsg = 'Update GUI config success!'
            } else {
                this.alertVariant = 'danger'
                this.alertMsg = 'Update GUI config failed!'
            }

            this.showAlert = true
        },
        async onSubmitPlayout (evt) {
            evt.preventDefault()
            await this.$store.dispatch('auth/inspectToken')
            const update = await this.$store.dispatch('config/setPlayoutConfig', this.configPlayout)

            if (update.status === 200) {
                this.alertVariant = 'success'
                this.alertMsg = 'Update playout config success!'
            } else {
                this.alertVariant = 'danger'
                this.alertMsg = 'Update playout config failed!'
            }

            this.showAlert = true
        },
        async onSubmitUser (evt) {
            evt.preventDefault()
            if (this.oldPass && this.newPass && this.newPass === this.confirmPass) {
                this.configUser.old_password = this.oldPass
                this.configUser.new_password = this.newPass
            }
            await this.$store.dispatch('auth/inspectToken')
            const update = await this.$store.dispatch('config/setUserConfig', this.configUser)

            if (update.status === 200) {
                this.alertVariant = 'success'
                this.alertMsg = 'Update user profil success!'
            } else {
                this.alertVariant = 'danger'
                this.alertMsg = 'Update user profil failed!'
            }

            this.showAlert = true

            this.oldPass = null
            this.newPass = null
            this.confirmPass = null
        },

        resetAlert () {
            this.showAlert = false
            this.alertVariant = 'success'
            this.alertMsg = ''
        }
    }
}
</script>

<style lang="scss">
.config-container {
    margin: 2em auto 2em auto;
    padding: 0;
}

.config-group {
    margin-bottom: 2em;
}

.input-field {
    max-width: 200px;
}

.text-area {
    overflow-y: hidden !important;
}
</style>
