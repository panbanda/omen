<template>
  <input ref="field" @keyup.enter="emit('submit')" />
</template>

<script setup lang="ts">
import { ref } from 'vue';

const emit = defineEmits<{ submit: [] }>();
const field = ref<HTMLInputElement | null>(null);
</script>
