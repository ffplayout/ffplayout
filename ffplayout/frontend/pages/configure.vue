<template>
    <div>
        <Menu />
        <b-card no-body>
            <b-tabs pills card vertical>
                <b-tab title="GUI" active>
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
                                    v-for="(prop, name, idx) in configGui[0]"
                                    :key="idx"
                                    label-cols-sm="2"
                                    :label="name"
                                    label-align-sm="right"
                                    :label-for="name"
                                >
                                    <b-form-select v-if="name === 'net_interface'" :id="name" v-model="configGui[0][name]" :options="netChoices" :value="prop" />
                                    <b-form-input v-else :id="name" v-model="configGui[0][name]" :value="prop" />
                                </b-form-group>
                            </b-form-group>
                            <b-button type="submit" variant="primary">
                                Save
                            </b-button>
                        </b-form>
                    </b-container>
                </b-tab>
                <b-tab title="Playout">
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
                                        input-id="tags-basic"
                                        separator=" ,;"
                                        :placeholder="`add ${name}...`"
                                        class="mb-2"
                                    />
                                    <b-form-input v-else :id="name" v-model="configPlayout[key][name]" :value="prop" />
                                </b-form-group>
                            </b-form-group>

                            <b-button type="submit" variant="primary">
                                Save
                            </b-button>
                        </b-form>
                    </b-container>
                </b-tab>
                <b-tab title="User">
                    <b-card-text>
                        <b-container class="config-container">
                            <b-form v-if="configUser" @submit="onSubmitGui">
                                <b-form-group
                                    label-cols-lg="2"
                                    label="User Configuration"
                                    label-size="lg"
                                    label-class="font-weight-bold pt-0"
                                    class="config-group"
                                >
                                    <b-form-group
                                        v-for="(prop, name, idx) in configUser[0]"
                                        :key="idx"
                                        label-cols-sm="2"
                                        :label="name"
                                        label-align-sm="right"
                                        :label-for="name"
                                    >
                                        <b-form-input v-if="name === 'username'" :id="name" v-model="configUser[0][name]" :value="prop" disabled />
                                        <b-form-input v-else :id="name" v-model="configUser[0][name]" :value="prop" />
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
                                <b-button type="submit" variant="primary">
                                    Save
                                </b-button>
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
            currentUser: store.state.config.currentUser,
            configUser: store.state.config.configUser,
            oldPass: null,
            newPass: null,
            confirmPass: null,
            showLogin: login
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
            this.$store.dispatch('config/setGuiConfig', this.configGui[0])
        },
        async onSubmitPlayout (evt) {
            evt.preventDefault()
            await this.$store.dispatch('auth/inspectToken')
            this.$store.dispatch('config/setPlayoutConfig', this.configPlayout)
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
