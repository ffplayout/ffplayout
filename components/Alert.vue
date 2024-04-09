<template>
    <div
        v-if="indexStore.showAlert"
        role="alert"
        class="alert fixed top-20 right-3 z-40 w-auto min-w-64 max-w-[90%] md:max-w-[50%] xl:max-w-[40%]"
        :class="`alert-${indexStore.alertVariant}`"
    >
        <SvgIcon :name="indexStore.alertVariant" />
        {{ indexStore.alertMsg }}
    </div>

    <!-- HACK: init alerts to get the right colors, can be remove when bug is fixed -->
    <div class="alert alert-success w-auto max-w-[700px] justify-start py-2 rounded hidden">
        <SvgIcon name="success" />
        <span class="truncate w-full">bla bla bla</span>
    </div>
    <div class="alert alert-warning w-auto max-w-[700px] justify-start py-2 rounded hidden">
        <SvgIcon name="warning" />
        <span class="truncate w-full">bla bla bla</span>
    </div>
    <div class="alert alert-info w-auto max-w-[700px] justify-start py-2 rounded hidden">
        <SvgIcon name="info" />
        <span class="truncate w-full">bla bla bla</span>
    </div>
    <div class="alert alert-error w-auto max-w-[700px] justify-start py-2 rounded hidden">
        <SvgIcon name="error" />
        <span class="truncate w-full">bla bla bla</span>
    </div>
</template>

<script setup lang="ts">
const indexStore = useIndex()
</script>
