<template>
  <input v-focus="query" v-bind="buildAttributes()">
  <p v-tooltip="describeTip"></p>
  <component :is="SearchBox" />
</template>

<script setup lang="ts">
import SearchBox from './SearchBox.vue';
import { vFocus } from './focus';

const query = '';
function buildAttributes() {
  return {};
}
function describeTip() {}
</script>
