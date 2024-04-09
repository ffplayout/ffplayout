<template>
    <div
        v-if="indexStore.showAlert"
        role="alert"
        class="alert fixed top-20 right-3 z-40 w-auto min-w-64 max-w-[90%] md:max-w-[50%] xl:max-w-[40%]"
        :class="`alert-${indexStore.alertVariant}`"
    >
        <div v-html="alertIcon(indexStore.alertVariant)" />
        {{ indexStore.alertMsg }}
    </div>

    <!-- HACK: init alerts to get the right colors, can be remove when bug is fixed -->
    <div class="alert alert-success w-auto max-w-[700px] justify-start py-2 rounded hidden">
        <div v-html="alertIcon('success')" />
        <span class="truncate w-full">bla bla bla</span>
    </div>
    <div class="alert alert-warning w-auto max-w-[700px] justify-start py-2 rounded hidden">
        <div v-html="alertIcon('warning')" />
        <span class="truncate w-full">bla bla bla</span>
    </div>
    <div class="alert alert-info w-auto max-w-[700px] justify-start py-2 rounded hidden">
        <div v-html="alertIcon('info')" />
        <span class="truncate w-full">bla bla bla</span>
    </div>
    <div class="alert alert-error w-auto max-w-[700px] justify-start py-2 rounded hidden">
        <div v-html="alertIcon('error')" />
        <span class="truncate w-full">bla bla bla</span>
    </div>
</template>

<script setup lang="ts">
const indexStore = useIndex()

function alertIcon(variance: string) {
    switch (variance) {
        case 'error':
            return `<svg
                    xmlns="http://www.w3.org/2000/svg"
                    class="stroke-current shrink-0 h-6 w-6"
                    fill="none"
                    viewBox="0 0 24 24"
                >
                    <path
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        stroke-width="2"
                        d="M10 14l2-2m0 0l2-2m-2 2l-2-2m2 2l2 2m7-2a9 9 0 11-18 0 9 9 0 0118 0z"
                    />
                </svg>`
        case 'success':
            return ` <svg
                    xmlns="http://www.w3.org/2000/svg"
                    class="stroke-current shrink-0 h-6 w-6"
                    fill="none"
                    viewBox="0 0 24 24"
                >
                    <path
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        stroke-width="2"
                        d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z"
                    />
                </svg>`
        case 'warning':
            return `<svg
                    xmlns="http://www.w3.org/2000/svg"
                    class="stroke-current shrink-0 h-6 w-6"
                    fill="none"
                    viewBox="0 0 24 24"
                >
                    <path
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        stroke-width="2"
                        d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"
                    />
                </svg>`
        default:
            return ` <svg
                    xmlns="http://www.w3.org/2000/svg"
                    fill="none"
                    viewBox="0 0 24 24"
                    class="stroke-current shrink-0 w-6 h-6"
                >
                    <path
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        stroke-width="2"
                        d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
                    />
                </svg>`
    }
}
</script>
