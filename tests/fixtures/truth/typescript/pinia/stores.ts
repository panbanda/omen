import { defineStore } from 'pinia';
import { extraActions } from './extra';

export const useUsersStore = defineStore('users', () => {
	const names: string[] = [];
	const addUser = (name: string) => {
		names.push(name);
	};
	function logout() {
		names.length = 0;
	}
	const hidden = () => names.length;
	return { names, addUser, logout, count: () => hidden() };
});

export const useCounterStore = defineStore('counter', {
	state: () => ({ total: 0 }),
	actions: {
		increment() {
			this.total += 1;
		},
	},
});

export const useMergedStore = defineStore('merged', () => {
	const save = () => true;
	const load = () => false;
	return { save, ...extraActions, load };
});
