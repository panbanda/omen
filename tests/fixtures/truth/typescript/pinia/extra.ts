export function makeSaver(): () => boolean {
	return () => false;
}

export const extraActions = { save: makeSaver() };
