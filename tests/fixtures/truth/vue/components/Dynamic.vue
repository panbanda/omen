<template>
  <component :is="current" />
</template>

<script setup lang="ts">
import SearchBox from './SearchBox.vue';

const current = SearchBox;
function helper() {}
</script>
