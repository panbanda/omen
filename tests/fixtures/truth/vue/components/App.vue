<template>
  <main :class="{ active: isActive }">
    <SearchBox v-model="query" @submit="search" />
    <Badge :count="total()" />
    <ul>
      <li v-for="(item, index) in items" :key="item.id" @click="select(item)">
        {{ index }}: {{ formatName(item.name) }}
      </li>
    </ul>
    <ItemList :rows="items">
      <template #row="{ row }">
        <span>{{ describe(row) }}</span>
      </template>
    </ItemList>
  </main>
</template>

<script setup lang="ts">
import { ref } from 'vue';
import SearchBox from './SearchBox.vue';
import ItemList from './ItemList.vue';
import { formatName } from './format';

const query = ref('');
const isActive = ref(true);
const items = ref([{ id: 1, name: 'a' }]);

function search() {}
function total() {
  return items.value.length;
}
function select(value: { id: number }) {
  return value.id;
}
function describe(value: unknown) {
  return String(value);
}
function Badge() {
  return null;
}
function item() {}
function row() {}
function unused() {}
</script>

<style scoped>
main {
  color: red;
}
</style>
