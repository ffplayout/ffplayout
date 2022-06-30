<template>
    <div>
        <div v-if="!$store.state.auth.isLogin">
            <div class="logout-div" />
            <b-container class="login-container">
                <div>
                    <div class="header">
                        <h1>ffplayout</h1>
                    </div>
                    <b-form class="login-form" @submit.prevent="login">
                        <b-form-group id="input-group-1" label="User:" label-for="input-user">
                            <b-form-input id="input-user" v-model="formUsername" type="text" required placeholder="Username" />
                        </b-form-group>
                        <b-form-group id="input-group-1" label="Password:" label-for="input-pass">
                            <b-form-input id="input-pass" v-model="formPassword" type="password" required placeholder="Password" />
                        </b-form-group>
                        <b-row>
                            <b-col cols="3">
                                <b-button type="submit" variant="primary">
                                    Login
                                </b-button>
                            </b-col>
                            <b-col cols="9">
                                <b-alert variant="danger" :show="showError" dismissible @dismissed="showError=false">
                                    {{ formError }}
                                </b-alert>
                            </b-col>
                        </b-row>
                    </b-form>
                </div>
            </b-container>
        </div>
        <div v-else>
            <b-container class="login-container">
                <div>
                    <div class="logo-div">
                        <b-img-lazy
                            src="/images/ffplayout.png"
                            alt="Logo"
                            fluid
                        />
                    </div>
                    <div class="actions">
                        <b-button-group class="actions-grp">
                            <b-button to="/player" variant="primary">
                                Player
                            </b-button>
                            <b-button to="/media" variant="primary">
                                Media
                            </b-button>
                            <b-button to="/message" variant="primary">
                                Message
                            </b-button>
                            <b-button to="logging" variant="primary">
                                Logging
                            </b-button>
                            <b-button to="/configure" variant="primary">
                                Configure
                            </b-button>
                            <b-button variant="primary" @click="logout()">
                                Logout
                            </b-button>
                        </b-button-group>
                    </div>
                </div>
            </b-container>
        </div>
    </div>
</template>

<script>
export default {
    components: {},

    data () {
        return {
            showError: false,
            formError: null,
            formUsername: '',
            formPassword: '',
            interval: null,
            stat: {}
        }
    },
    created () {
        this.init()
    },
    beforeDestroy () {
        clearInterval(this.interval)
    },
    methods: {
        async init () {
            await this.$store.dispatch('auth/inspectToken')
        },
        async login () {
            try {
                const status = await this.$store.dispatch('auth/obtainToken', {
                    username: this.formUsername,
                    password: this.formPassword
                })
                this.formUsername = ''
                this.formPassword = ''
                this.formError = null

                if (status === 401 || status === 400) {
                    this.formError = 'Wrong user or password!'
                    this.showError = true
                }

                await this.$store.dispatch('config/nuxtClientInit')
            } catch (e) {
                this.formError = e.message
            }
        },
        logout () {
            clearInterval(this.interval)

            try {
                this.$store.commit('auth/REMOVE_TOKEN')
                this.$store.commit('auth/UPDATE_IS_LOGIN', false)
            } catch (e) {
                this.formError = e.message
            }
        }
    }
}
</script>

<style>
.login-container {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100vh;
}

.header {
    text-align: center;
    margin-bottom: 3em;
}

.logo-div {
    width: 100%;
    text-align: center;
    margin-bottom: 5em;
}

.login-form {
    min-width: 300px;
}

.manage-btn {
    margin: 0 auto 0 auto;
}

.chart-col {
    text-align: center;
    min-width: 10em;
    min-height: 15em;
    border: solid #c3c3c3;
}

.stat-div {
    padding-top: .5em;
    position: relative;
    height: 12em;
}

.stat-center {
    margin: 0;
    position: absolute;
    width: 100%;
    top: 50%;
    -ms-transform: translateY(-50%);
    transform: translateY(-50%);
}

.chart1 {
    background: rgba(210, 85, 23, 0.1);
}
.chart2 {
    background: rgba(122, 210, 23, 0.1);
}
.chart3 {
    background: rgba(23, 210, 149, 0.1);
}
.chart4 {
    background: rgba(23, 160, 210, 0.1);
}
.chart5 {
    background: rgba(122, 23, 210, 0.1);
}
.chart6 {
    background: rgba(210, 23, 74, 0.1);
}

.actions {
    text-align: center;
    margin-top: 1em;
}

@media (max-width: 380px) {
    .actions-grp {
        display: flex;
        flex-direction: column;
        margin: 0 2em 0 2em;
    }
}
</style>
