<template>
    <div>
        <Menu />
        <b-row class="date-row">
            <b-col>
                <b-datepicker v-model="listDate" size="sm" class="date-div" offset="-35px" />
            </b-col>
        </b-row>
        <b-container class="log-container">
            <div
                v-if="currentLog"
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
                /* eslint-disable no-control-regex */
                .replace(/\x1B\[33m(.*?)\x1B\[0m/g, '<span class="log-number">$1</span>')
                .replace(/\x1B\[1m\x1B\[35m(.*?)\x1B\[0m\x1B\[22m/g, '<span class="log-addr">$1</span>')
                .replace(/\x1B\[94m(.*?)\x1B\[0m/g, '<span class="log-cmd">$1</span>')
                .replace(/\x1B\[90m(.*?)\x1B\[0m/g, '<span class="log-debug">$1</span>')
                .replace(/(\[\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}.[\d]+\])/g, '<span class="log-time">$1</span>')
                .replace(/\[ INFO\]/g, '<span class="log-info">[ INFO]</span>')
                .replace(/\[ WARN\]/g, '<span class="log-warning">[ WARN]</span>')
                .replace(/\[ERROR\]/g, '<span class="log-error">[ERROR]</span>')
                .replace(/\[DEBUG\]/g, '<span class="log-debug">[DEBUG]</span>')
                .replace(/\[Decoder\]/g, '<span class="log-decoder">[Decoder]</span>')
                .replace(/\[Encoder\]/g, '<span class="log-encoder">[Encoder]</span>')
                .replace(/\[Server\]/g, '<span class="log-server">[Server]</span>')
                .replace(/\[Validator\]/g, '<span class="log-server">[Validator]</span>')
        }
    },

    middleware: 'auth',

    data () {
        return {
            currentLog: null,
            listDate: this.$dayjs().utcOffset(0).format('YYYY-MM-DD')
        }
    },

    computed: {
        ...mapState('config', ['configID', 'utcOffset'])
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
        this.listDate = this.$dayjs().utcOffset(this.utcOffset).format('YYYY-MM-DD')
        await this.getLog()
    },

    methods: {
        async getLog () {
            let date = this.listDate

            if (date === this.$dayjs().utcOffset(this.utcOffset).format('YYYY-MM-DD')) {
                date = ''
            }

            const response = await this.$axios.get(
                `api/log/${this.$store.state.config.configGui[this.$store.state.config.configID].id}?date=${date}`)

            if (response.data) {
                this.currentLog = response.data
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

.log-time {
    color: #666864;
}

.log-number {
    color: #e2c317;
}

.log-addr {
    color: #ad7fa8;
    font-weight: 500;
}

.log-cmd {
    color: #6c95c2;
}

.log-content {
    color: #ececec;
    width: 100%;
    height: 100%;
    font-family: monospace;
    font-size: 13px;
    white-space: pre;
    overflow: scroll;
    scrollbar-width: medium;
}

.log-info {
    color: #8ae234;
}

.log-warning {
    color: #ff8700;
}

.log-error {
    color: #d32828;
}

.log-debug {
    color: #6e99c7;
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

</style>
