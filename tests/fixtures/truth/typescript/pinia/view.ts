import { useCounterStore, useMergedStore, useUsersStore } from './stores';

export function addAda(): void {
	const usersStore = useUsersStore();
	usersStore.addUser('ada');
	// @ts-expect-error The store does not return `hidden`.
	usersStore.hidden();
}

export function signOut(): void {
	useUsersStore().logout();
}

export function countUp(): void {
	const counter = useCounterStore();
	counter.increment();
	counter.$reset();
}

export function merge(): void {
	const merged = useMergedStore();
	merged.save();
	merged.load();
}
