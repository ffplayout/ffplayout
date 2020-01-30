<template>
    <div>

    </div>
</template>

<script>
export default {
    components: {},

    created () {
        this.getPath()
    },

    methods: {
        async getPath () {
            await this.$store.dispatch('auth/inspectToken')
            // const response = await this.$axios.get('apt/')
        }
    }
}
</script>

<style>

</style>
