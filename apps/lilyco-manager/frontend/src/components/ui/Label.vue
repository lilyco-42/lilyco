<script setup lang="ts">
defineProps<{for?:string}>()
</script>
<template>
 <label :for="$props.for" class="text-xs font-semibold text-foreground"><slot/></label>
</template>
