<template>
  <div v-for="row in rows" :key="row.id">
    <slot name="row" :row="row" />
  </div>
</template>

<script setup lang="ts">
defineProps<{ rows: { id: number }[] }>();
</script>
