<template>
    <div>
        <Menu />
        <b-row class="date-row">
            <b-col>
                <b-datepicker v-model="listDate" size="sm" class="date-div" offset="-35px" />
            </b-col>
        </b-row>
        <b-container class="log-container">
            <!-- eslint-disable-next-line -->
            <perfect-scrollbar
                v-if="currentLog"
                :options="scrollOP"
                class="log-content"
                :inner-html.prop="currentLog | formatStr"
            />
        </b-container>
    </div>
</template>

<script>
import { mapState } from 'vuex'
import Menu from '@/components/Menu.vue'

export default {
    name: 'Logging',

    components: {
        Menu
    },

    filters: {
        formatStr (text) {
            return text
                .replace(/(".*")/g, '<span class="log-cmd">$1</span>')
                .replace(/(?<!".*)(\/.*)/g, '<span class="log-path">$1</span>')
                .replace(/(\/[\w\d.\-/]+\n)/g, '<span class="log-path">$1</span>')
                .replace(/((tcp|https?):\/\/[\w\d.:]+)/g, '<span class="log-url">$1</span>')
                .replace(/(\[\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}[0-9,.]+\])/g, '<span class="log-time">$1</span>')
                .replace(/\[INFO\]/g, '<span class="log-info">[INFO]</span>')
                .replace(/\[WARNING\]/g, '<span class="log-warning">[WARNING]</span>')
                .replace(/\[ERROR\]/g, '<span class="log-error">[ERROR]</span>')
                .replace(/\[DEBUG\]/g, '<span class="log-debug">[DEBUG]</span>')
                .replace(/\[Decoder\]/g, '<span class="log-decoder">[Decoder]</span>')
                .replace(/\[Encoder\]/g, '<span class="log-encoder">[Encoder]</span>')
                .replace(/\[Server\]/g, '<span class="log-server">[Server]</span>')
        }
    },

    middleware: 'auth',

    data () {
        return {
            currentLog: null,
            listDate: this.$dayjs().tz(this.timezone).format('YYYY-MM-DD'),
            scrollOP: {
                wheelSpeed: 5,
                minScrollbarLength: 30
            }
        }
    },

    computed: {
        ...mapState('config', ['configID', 'timezone'])
    },

    watch: {
        listDate () {
            this.getLog()
        },

        configID () {
            this.getLog()
        }
    },

    async created () {
        await this.getLog()
    },

    methods: {
        async getLog () {
            const response = await this.$axios.get(
                `api/player/log/?date=${this.listDate}&channel=${this.$store.state.config.configGui[this.$store.state.config.configID].id}`)

            if (response.data.log) {
                this.currentLog = response.data.log
            } else {
                this.currentLog = ''
            }
        }
    }
}
</script>

<style>
.ps__thumb-x {
    display: inherit !important;
}

.log-container {
    background: #1d2024;
    max-width: 99%;
    width: 99%;
    height: calc(100% - 90px);
    padding: 1em;
    overflow: hidden
}

.log-content {
    color: #ececec;
    width: 100%;
    height: 100%;
    font-family: monospace;
    font-size: 13px;
    white-space: pre;
}

.log-time {
    color: #a7a7a7;
}

.log-info {
    color: #51d1de;
}

.log-warning {
    color: #e4a428;
}

.log-error {
    color: #e42e28;
}

.log-debug {
    color: #23e493;
}

.log-decoder {
    color: #56efff;
}

.log-encoder {
    color: #45ccee;
}

.log-server {
    color: #23cbdd;
}

.log-path {
    color: #e366cf;
}

.log-url {
    color: #e3d666;
}

.log-cmd {
    color: #f1aa77;
}
</style>
