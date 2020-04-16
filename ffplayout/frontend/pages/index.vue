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
                        <p v-if="formError" class="error">
                            {{ formError }}
                        </p>
                        <b-form-group id="input-group-1" label="User:" label-for="input-user">
                            <b-form-input id="input-user" v-model="formUsername" type="text" required placeholder="Username" />
                        </b-form-group>
                        <b-form-group id="input-group-1" label="Password:" label-for="input-pass">
                            <b-form-input id="input-pass" v-model="formPassword" type="password" required placeholder="Password" />
                        </b-form-group>
                        <b-button type="submit" variant="primary">
                            Login
                        </b-button>
                    </b-form>
                </div>
            </b-container>
        </div>
        <div v-else>
            <b-container class="login-container">
                <div>
                    <b-row cols="3">
                        <b-col cols="4" class="chart-col chart1">
                            <br>
                            <div class="stat-div">
                                <div class="stat-center" style="text-align: left;">
                                    <h1>ffplayout</h1>
                                    <h3 v-if="stat.system">
                                        {{ stat.system }}<br>
                                        {{ stat.node }}<br>
                                        {{ stat.machine }}
                                    </h3>
                                </div>
                            </div>
                        </b-col>
                        <b-col cols="4" class="chart-col chart2">
                            <div v-if="stat.cpu_usage">
                                <div>
                                    <strong>CPU</strong>
                                </div>
                                <div class="stat-div">
                                    <div class="stat-center">
                                        <b-progress :value="stat.cpu_usage" max="100" variant="success" height="1rem" />
                                        <br>
                                        <div style="text-align: left;">
                                            <strong>Usage: </strong>{{ stat.cpu_usage }}%<br>
                                            <strong>Load: </strong> {{ stat.cpu_load[0] }} {{ stat.cpu_load[1] }} {{ stat.cpu_load[2] }}
                                        </div>
                                    </div>
                                </div>
                            </div>
                        </b-col>
                        <b-col cols="4" class="chart-col chart3">
                            <div v-if="stat.ram_total">
                                <div>
                                    <strong>RAM</strong>
                                </div>
                                <div class="stat-div">
                                    <div class="stat-center">
                                        <div style="text-align: left;">
                                            <strong>Total: </strong> {{ stat.ram_total[1] }}<br>
                                            <strong>Used: </strong> {{ stat.ram_used[1] }}<br>
                                            <strong>Free: </strong> {{ stat.ram_free[1] }}<br>
                                            <strong>Cached: </strong> {{ stat.ram_cached[1] }}
                                        </div>
                                    </div>
                                </div>
                            </div>
                        </b-col>
                        <b-col cols="4" class="chart-col chart4">
                            <div v-if="stat.swap_total">
                                <div>
                                    <strong>SWAP</strong>
                                </div>
                                <div class="stat-div">
                                    <div class="stat-center">
                                        <div style="text-align: left;">
                                            <strong>Total: </strong> {{ stat.swap_total[1] }}<br>
                                            <strong>Used: </strong> {{ stat.swap_used[1] }}<br>
                                            <strong>Free: </strong> {{ stat.swap_free[1] }}
                                        </div>
                                    </div>
                                </div>
                            </div>
                        </b-col>
                        <b-col cols="4" class="chart-col chart5">
                            <div v-if="stat.disk_total">
                                <div>
                                    <strong>DISK</strong>
                                </div>
                                <div class="stat-div">
                                    <div class="stat-center">
                                        <div style="text-align: left;">
                                            <strong>Total: </strong> {{ stat.disk_total[1] }}<br>
                                            <strong>Used: </strong> {{ stat.disk_used[1] }}<br>
                                            <strong>Free: </strong> {{ stat.disk_free[1] }}
                                        </div>
                                    </div>
                                </div>
                            </div>
                        </b-col>
                        <b-col cols="4" class="chart-col chart6">
                            <div v-if="stat.net_send">
                                <div>
                                    <strong>NET</strong>
                                </div>
                                <div class="stat-div">
                                    <div class="stat-center">
                                        <div style="text-align: left;">
                                            <strong>Download: </strong> {{ stat.net_speed_recv[1] }}/s<br>
                                            <strong>Upload: </strong> {{ stat.net_speed_send[1] }}/s<br>
                                            <strong>Downloaded: </strong> {{ stat.net_recv[1] }}<br>
                                            <strong>Uploaded: </strong> {{ stat.net_send[1] }}<br>
                                            <strong>Recived Errors: </strong> {{ stat.net_errin }}<br>
                                            <strong>Sended Errors: </strong> {{ stat.net_errout }}
                                        </div>
                                    </div>
                                </div>
                            </div>
                        </b-col>
                    </b-row>

                    <div class="actions">
                        <b-button-group class="actions-grp">
                            <b-button to="/control" variant="primary">
                                Control
                            </b-button>
                            <b-button to="/media" variant="primary">
                                Media
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
        if (this.interval) {
            clearInterval(this.interval)
        }
    },
    methods: {
        async init () {
            await this.$store.dispatch('auth/inspectToken')
            this.checkLogin()
        },
        async login () {
            try {
                await this.$store.dispatch('auth/obtainToken', {
                    username: this.formUsername,
                    password: this.formPassword
                })
                this.formUsername = ''
                this.formPassword = ''
                this.formError = null

                this.checkLogin()
            } catch (e) {
                this.formError = e.message
            }
        },
        async logout () {
            try {
                await this.$store.commit('auth/REMOVE_TOKEN')
                await this.$store.commit('auth/UPDATE_IS_LOGIN', false)
            } catch (e) {
                this.formError = e.message
            }
        },
        checkLogin () {
            if (this.$store.state.auth.isLogin) {
                // this.$router.push('/player')
                this.sysStats()
            }
        },

        async sysStats () {
            const response = await this.$axios.get('api/stats/?stats=all', { headers: { Authorization: 'Bearer ' + this.$store.state.auth.jwtToken }, progress: false })
            this.stat = response.data

            this.interval = setInterval(async () => {
                await this.$store.dispatch('auth/inspectToken')
                const response = await this.$axios.get('api/stats/?stats=all', { headers: { Authorization: 'Bearer ' + this.$store.state.auth.jwtToken }, progress: false })
                this.stat = response.data
            }, 2000)
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
