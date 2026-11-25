//! Calls of Pinia stores. See `crate::index::stores`.
//!
//! - A call of the function of a store is external: Pinia gives the store.
//! - A member of a store value is the function of its key. A store value is
//!   a direct call of the function of a store, or a `const` local name
//!   without a declared type whose value is that call.
//! - A function of the Pinia API on a store value is external.
//! - Each other member of a store value abstains, also when a method of the
//!   repository has the name. A plugin can add members to each store.

use std::collections::{BTreeSet, HashMap};

use super::{Basis, Id, ImportKind, Local, Outcome, ReferenceKind, Resolver, Status};

/// The functions of the Pinia API that each store has.
const PINIA_API: [&str; 5] = ["$dispose", "$onAction", "$patch", "$reset", "$subscribe"];

/// A store: the position of its file and its position in `FileSymbols::stores`.
pub(super) type StoreId = (usize, usize);

/// For each file, its stores by each name under which the file exports them.
/// `None` means that no file has a store.
pub(super) type StoreNames<'a> = Option<Vec<HashMap<&'a str, usize>>>;

impl<'a> Resolver<'a> {
    fn store_names(&self) -> Option<&Vec<HashMap<&'a str, usize>>> {
        self.store_names
            .get_or_init(|| {
                if self.files.iter().all(|facts| facts.stores.is_empty()) {
                    return None;
                }
                let names = self
                    .files
                    .iter()
                    .map(|facts| {
                        let mut names = HashMap::new();
                        for (position, store) in facts.stores.iter().enumerate() {
                            names.insert(store.name.as_str(), position);
                            for alias in &store.aliases {
                                names.insert(alias.as_str(), position);
                            }
                        }
                        names
                    })
                    .collect();
                Some(names)
            })
            .as_ref()
    }

    /// The stores that a module gives under `name`, itself or through its re-exports.
    fn exported_stores(
        &self,
        file: usize,
        name: &str,
        seen: &mut BTreeSet<(usize, String)>,
    ) -> Vec<StoreId> {
        if !seen.insert((file, name.to_string())) {
            return Vec::new();
        }
        let names = self.store_names().map(|names| &names[file]);
        if let Some(position) = names.and_then(|names| names.get(name)) {
            return vec![(file, *position)];
        }
        let mut found = Vec::new();
        for binding in &self.files[file].imports {
            let source = match binding.kind {
                ImportKind::ReExportNamed if binding.local_name.as_deref() == Some(name) => {
                    binding.imported_name.as_deref().unwrap_or(name)
                }
                ImportKind::ReExportWildcard => name,
                _ => continue,
            };
            for target in self.target_files(binding) {
                found.extend(self.exported_stores(target, source, seen));
            }
        }
        found
    }

    /// The store whose function the bare name `name` at the byte `at` names.
    fn store_named(&self, file: usize, name: &str, chain: &[Id], at: usize) -> Option<StoreId> {
        // A local name or a definition of the file with the name hides the import.
        if self.is_local_value(chain, file, name, at) || self.named_in(name, file).next().is_some()
        {
            return None;
        }
        if let Ok(position) = self.files[file]
            .stores
            .binary_search_by(|store| store.name.as_str().cmp(name))
        {
            return Some((file, position));
        }
        let binding = self.imports_named(file, name).next()?;
        if binding.kind != ImportKind::Named {
            return None;
        }
        let imported = binding.imported_name.as_deref().unwrap_or(name);
        let query = (file, name.to_string());
        if let Some(found) = self.stores_cache.read().get(&query) {
            return *found;
        }
        let mut found: Vec<StoreId> = self
            .target_files(binding)
            .into_iter()
            .flat_map(|target| self.exported_stores(target, imported, &mut BTreeSet::new()))
            .collect();
        found.sort();
        found.dedup();
        let found = match found.as_slice() {
            [only] => Some(*only),
            _ => None,
        };
        self.stores_cache.write().insert(query, found);
        found
    }

    /// The store that the call at `position` gives: a bare call of the
    /// function of a store.
    pub(super) fn store_call(&self, file: usize, position: usize) -> Option<StoreId> {
        self.store_names()?;
        let facts = &self.files[file];
        let reference = &facts.references[position];
        if reference.kind != ReferenceKind::Call
            || reference.receiver.is_some()
            || reference.qualifier.is_some()
        {
            return None;
        }
        let site = &facts.reference_facts[position];
        let chain = self.chain(file, site.owner);
        self.store_named(file, &reference.name, &chain, site.start_byte)
    }

    /// The store that a local name holds. The value must be the call of the
    /// function of a store and nothing else, and the name cannot get a
    /// different value.
    pub(super) fn store_of_local(&self, local: &Local, file: usize) -> Option<StoreId> {
        let name = &local.name;
        if !name.constant || name.type_name.is_some() || name.element || name.result_index.is_some()
        {
            return None;
        }
        match self.references_at(file, name.value_start?) {
            [only] => self.store_call(file, *only),
            _ => None,
        }
    }

    /// The outcome of the call of the member `name` of a store value.
    pub(super) fn store_member(&self, store: StoreId, name: &str) -> Outcome {
        if PINIA_API.contains(&name) {
            return Outcome::none(Status::External);
        }
        let members = &self.files[store.0].stores[store.1].members;
        members
            .get(name)
            .and_then(|position| {
                Outcome::of(Basis::ReceiverDeclaredType, vec![(store.0, *position)])
            })
            .unwrap_or_else(|| Outcome::none(Status::Unresolved))
    }
}

#[cfg(test)]
mod tests {
    use crate::index::resolve::testing::rows;

    const EXTENSIONS: [&str; 4] = ["ts", "js", "tsx", "jsx"];

    /// A setup store with each form of a returned member, and a store of
    /// the options form.
    fn store_file() -> String {
        "import { defineStore } from 'pinia';\n\
         import { ref } from 'vue';\n\
         \n\
         export const useUsersStore = defineStore('users', () => {\n\
         \tconst users = ref([]);\n\
         \tconst addUsers = (list) => {\n\
         \t\tusers.value.push(...list);\n\
         \t};\n\
         \tasync function logout() {\n\
         \t\tusers.value = [];\n\
         \t}\n\
         \tconst hidden = () => 1;\n\
         \treturn { users, addUsers, logout, signOut: logout, refresh() { return 1; } };\n\
         });\n\
         \n\
         export const useCounterStore = defineStore('counter', {\n\
         \tstate: () => ({ count: 0 }),\n\
         \tgetters: { double: (state) => state.count * 2 },\n\
         \tactions: {\n\
         \t\tincrement() { this.count++; },\n\
         \t},\n\
         });\n"
            .to_string()
    }

    /// A class whose methods have the names of the store members. A call
    /// that the store rules do not decide offers these methods by name.
    const OTHER: &str = "export class AuthController {\n\
         \tlogout() {}\n\
         \thidden() {}\n\
         \tusers() {}\n\
         \tdouble() {}\n\
         \tpersist() {}\n\
         }\n";

    fn rows_of(extension: &str, view: &str) -> Vec<String> {
        let store = format!("users.store.{extension}");
        let other = format!("auth.{extension}");
        let file = format!("view.{extension}");
        let store_source = store_file();
        rows(
            &[
                (store.as_str(), store_source.as_str()),
                (other.as_str(), OTHER),
                (file.as_str(), view),
            ],
            &file,
        )
    }

    #[test]
    fn test_members_of_a_local_store_value() {
        let view = "import { useUsersStore } from './users.store';\n\
             \n\
             export function view() {\n\
             \tconst usersStore = useUsersStore();\n\
             \tusersStore.logout();\n\
             \tusersStore.signOut();\n\
             \tusersStore.addUsers([]);\n\
             \tusersStore.refresh();\n\
             }\n";
        for extension in EXTENSIONS {
            let store = format!("users.store.{extension}");
            assert_eq!(
                rows_of(extension, view),
                [
                    "4 view>useUsersStore External - []".to_string(),
                    format!("5 view>usersStore.logout UniqueCandidate ReceiverDeclaredType [{store}#logout]"),
                    format!("6 view>usersStore.signOut UniqueCandidate ReceiverDeclaredType [{store}#logout]"),
                    format!("7 view>usersStore.addUsers UniqueCandidate ReceiverDeclaredType [{store}#addUsers]"),
                    format!("8 view>usersStore.refresh UniqueCandidate ReceiverDeclaredType [{store}#refresh]"),
                ],
                "{extension}"
            );
        }
    }

    #[test]
    fn test_members_of_a_direct_store_call() {
        let view = "import { useUsersStore, useCounterStore } from './users.store';\n\
             \n\
             export function view() {\n\
             \tuseUsersStore().logout();\n\
             \tuseCounterStore().increment();\n\
             }\n";
        for extension in EXTENSIONS {
            let store = format!("users.store.{extension}");
            assert_eq!(
                rows_of(extension, view),
                [
                    format!("4 view>(..).logout UniqueCandidate ReceiverDeclaredType [{store}#logout]"),
                    "4 view>useUsersStore External - []".to_string(),
                    format!("5 view>(..).increment UniqueCandidate ReceiverDeclaredType [{store}#increment]"),
                    "5 view>useCounterStore External - []".to_string(),
                ],
                "{extension}"
            );
        }
    }

    /// A member that the store does not give, a member that is not a
    /// function, and a getter abstain. A member of the Pinia API is external.
    #[test]
    fn test_other_members_of_a_store_abstain() {
        let view = "import { useUsersStore, useCounterStore } from './users.store';\n\
             \n\
             export function view() {\n\
             \tconst usersStore = useUsersStore();\n\
             \tusersStore.hidden();\n\
             \tusersStore.users();\n\
             \tusersStore.persist();\n\
             \tusersStore.$reset();\n\
             \tuseCounterStore().double();\n\
             \tuseCounterStore().$patch({});\n\
             }\n";
        for extension in EXTENSIONS {
            assert_eq!(
                rows_of(extension, view),
                [
                    "4 view>useUsersStore External - []",
                    "5 view>usersStore.hidden Unresolved - []",
                    "6 view>usersStore.users Unresolved - []",
                    "7 view>usersStore.persist Unresolved - []",
                    "8 view>usersStore.$reset External - []",
                    "9 view>(..).double Unresolved - []",
                    "9 view>useCounterStore External - []",
                    "10 view>(..).$patch External - []",
                    "10 view>useCounterStore External - []",
                ],
                "{extension}"
            );
        }
    }

    /// A spread after a key can give a different value to the key. A key
    /// after the spread keeps its value.
    #[test]
    fn test_a_spread_abstains_for_the_keys_before_it() {
        let store = "import { defineStore } from 'pinia';\n\
             import { extra } from './extra';\n\
             \n\
             export const useStore = defineStore('s', () => {\n\
             \tconst save = () => 1;\n\
             \tconst load = () => 2;\n\
             \treturn { save, ...extra, load };\n\
             });\n";
        let view = "import { useStore } from './store';\n\
             \n\
             export function view() {\n\
             \tconst store = useStore();\n\
             \tstore.save();\n\
             \tstore.load();\n\
             }\n";
        let other = "export const extra = {};\nexport class Saver {\n\tsave() {}\n}\n";
        assert_eq!(
            rows(
                &[("store.ts", store), ("extra.ts", other), ("view.ts", view)],
                "view.ts"
            ),
            [
                "4 view>useStore External - []",
                "5 view>store.save Unresolved - []",
                "6 view>store.load UniqueCandidate ReceiverDeclaredType [store.ts#load]",
            ]
        );
    }

    /// The members of a setup function with more than one `return` depend
    /// on the path.
    #[test]
    fn test_a_setup_with_several_returns_abstains() {
        let store = "import { defineStore } from 'pinia';\n\
             \n\
             export const useStore = defineStore('s', () => {\n\
             \tconst save = () => 1;\n\
             \tif (Math.random() > 0.5) {\n\
             \t\treturn { save };\n\
             \t}\n\
             \treturn { save };\n\
             });\n";
        let view = "import { useStore } from './store';\n\
             \n\
             export function view() {\n\
             \tuseStore().save();\n\
             }\n";
        let other = "export class Saver {\n\tsave() {}\n}\n";
        assert_eq!(
            rows(
                &[("store.ts", store), ("other.ts", other), ("view.ts", view)],
                "view.ts"
            ),
            [
                "4 view>(..).save Unresolved - []",
                "4 view>useStore External - []"
            ]
        );
    }

    /// The rules apply only to `defineStore` of Pinia, to a store value
    /// without a declared type, and to a name that the store binds.
    #[test]
    fn test_other_values_do_not_use_the_store_rules() {
        let store = "import { defineStore } from './local';\n\
             \n\
             export const useStore = defineStore('s', () => {\n\
             \tconst logout = () => 1;\n\
             \treturn { logout };\n\
             });\n";
        let pinia = "import { defineStore } from 'pinia';\n\
             \n\
             export const usePinia = defineStore('p', () => {\n\
             \tconst logout = () => 1;\n\
             \treturn { logout };\n\
             });\n";
        let view = "import { useStore } from './store';\n\
             import { usePinia } from './pinia';\n\
             import { AuthController } from './auth';\n\
             \n\
             export function view(usePinia2) {\n\
             \tuseStore().logout();\n\
             \tconst typed: AuthController = usePinia();\n\
             \ttyped.logout();\n\
             \tlet changed = usePinia();\n\
             \tchanged.logout();\n\
             }\n\
             \n\
             export function shadowed(usePinia) {\n\
             \tusePinia().logout();\n\
             }\n";
        assert_eq!(
            rows(
                &[
                    ("local.ts", "export function defineStore(id, setup) {}\n"),
                    ("store.ts", store),
                    ("pinia.ts", pinia),
                    ("auth.ts", OTHER),
                    ("view.ts", view),
                ],
                "view.ts"
            ),
            [
                "6 view>(..).logout Ambiguous NameOnly [auth.ts#AuthController.logout]",
                "6 view>useStore Unresolved - []",
                "7 view>usePinia External - []",
                "8 view>typed.logout UniqueCandidate ReceiverDeclaredType [auth.ts#AuthController.logout]",
                "9 view>usePinia External - []",
                "10 view>changed.logout Ambiguous NameOnly [auth.ts#AuthController.logout]",
                "14 shadowed>(..).logout Ambiguous NameOnly [auth.ts#AuthController.logout]",
                "14 shadowed>usePinia Unresolved - []",
            ]
        );
    }

    /// A barrel module gives the store again, by name and with a wildcard.
    #[test]
    fn test_a_store_through_re_exports() {
        let store = "import { defineStore as define } from 'pinia';\n\
             \n\
             export const useStore = define('s', function () {\n\
             \tfunction save() {}\n\
             \treturn { save };\n\
             });\n";
        let view = "import { useStore } from './index';\n\
             import { useStore as useAgain } from './named';\n\
             \n\
             export function view() {\n\
             \tuseStore().save();\n\
             \tuseAgain().save();\n\
             }\n";
        assert_eq!(
            rows(
                &[
                    ("store.ts", store),
                    ("index.ts", "export * from './store';\n"),
                    ("named.ts", "export { useStore } from './store';\n"),
                    ("view.ts", view),
                ],
                "view.ts"
            ),
            [
                "5 view>(..).save UniqueCandidate ReceiverDeclaredType [store.ts#save]",
                "5 view>useStore External - []",
                "6 view>(..).save UniqueCandidate ReceiverDeclaredType [store.ts#save]",
                "6 view>useAgain External - []",
            ]
        );
    }

    /// Each form of a key, and the forms that abstain: a `let`, a key that
    /// a computed key can replace, and an accessor.
    #[test]
    fn test_forms_of_the_keys_of_a_store() {
        let store = "import { defineStore } from 'pinia';\n\
             \n\
             const useOptions = defineStore({\n\
             \tid: 'options',\n\
             \tactions: {\n\
             \t\tsave: function () {},\n\
             \t\t'load': async () => {},\n\
             \t\tget total() { return 1; },\n\
             \t},\n\
             });\n\
             \n\
             const useSetup = defineStore('setup', () => {\n\
             \tlet changed = () => 1;\n\
             \tfunction kept() {}\n\
             \tconst key = 'kept';\n\
             \treturn ({ changed, kept, [key]: changed, after: () => 2 });\n\
             });\n\
             \n\
             export { useOptions as useRenamed, useSetup };\n";
        let view = "import { useRenamed, useSetup } from './store';\n\
             \n\
             export function view() {\n\
             \tconst options = useRenamed();\n\
             \toptions.save();\n\
             \toptions.load();\n\
             \toptions.total();\n\
             \tconst setup = useSetup();\n\
             \tsetup.changed();\n\
             \tsetup.kept();\n\
             \tsetup.after();\n\
             }\n";
        assert_eq!(
            rows(&[("store.ts", store), ("view.ts", view)], "view.ts"),
            [
                "4 view>useRenamed External - []",
                "5 view>options.save UniqueCandidate ReceiverDeclaredType [store.ts#save]",
                // The extraction gives no symbol for a function with a string key.
                "6 view>options.load Unresolved - []",
                "7 view>options.total Unresolved - []",
                "8 view>useSetup External - []",
                "9 view>setup.changed Unresolved - []",
                "10 view>setup.kept Unresolved - []",
                "11 view>setup.after UniqueCandidate ReceiverDeclaredType [store.ts#after]",
            ]
        );
    }

    /// A store of one file, used in a function of a different store.
    #[test]
    fn test_a_store_value_in_a_setup_function() {
        let store = "import { defineStore } from 'pinia';\n\
             \n\
             export const useUsersStore = defineStore('users', () => {\n\
             \tconst logout = () => 1;\n\
             \treturn { logout };\n\
             });\n\
             \n\
             export const useAuthStore = defineStore('auth', () => {\n\
             \tconst usersStore = useUsersStore();\n\
             \tconst signOut = () => usersStore.logout();\n\
             \treturn { signOut };\n\
             });\n";
        assert_eq!(
            rows(&[("store.ts", store), ("auth.ts", OTHER)], "store.ts"),
            [
                "3 ->defineStore External - []",
                "8 ->defineStore External - []",
                "9 ->useUsersStore External - []",
                "10 signOut>usersStore.logout UniqueCandidate ReceiverDeclaredType [store.ts#logout]",
            ]
        );
    }
}

#[cfg(test)]
mod timing_tests {
    use crate::index::resolve::testing::rows;
    use crate::index::timing::{assert_scales_linearly, Stopwatch};

    /// One file with `size` stores, and one file that imports each store and
    /// calls one member of each.
    fn stores_in_one_file_at(size: usize) -> std::time::Duration {
        let mut stores = String::from("import { defineStore } from 'pinia';\n");
        let mut names = Vec::new();
        let mut calls = String::new();
        for position in 0..size {
            stores.push_str(&format!(
                "export const useStore{position} = defineStore('s{position}', () => {{\n\
                 \tconst run{position} = () => {position};\n\
                 \treturn {{ run{position} }};\n\
                 }});\n"
            ));
            names.push(format!("useStore{position}"));
            calls.push_str(&format!(
                "\tconst store{position} = useStore{position}();\n\tstore{position}.run{position}();\n"
            ));
        }
        let view = format!(
            "import {{ {} }} from './stores';\n\nexport function view() {{\n{calls}}}\n",
            names.join(", ")
        );
        let started = Stopwatch::start();
        let found = rows(&[("stores.ts", &stores), ("view.ts", &view)], "view.ts");
        let elapsed = started.elapsed();
        assert_eq!(found.len(), 2 * size);
        let last = size - 1;
        assert_eq!(
            found[2 * size - 1],
            format!(
                "{} view>store{last}.run{last} UniqueCandidate ReceiverDeclaredType [stores.ts#run{last}]",
                4 + 2 * last + 1
            )
        );
        elapsed
    }

    /// One store with `size` members, and one call of each member.
    fn members_of_one_store_at(size: usize) -> std::time::Duration {
        let mut body = String::new();
        let mut keys = Vec::new();
        let mut calls = String::new();
        for position in 0..size {
            body.push_str(&format!("\tconst run{position} = () => {position};\n"));
            keys.push(format!("run{position}"));
            calls.push_str(&format!("\tstore.run{position}();\n"));
        }
        let stores = format!(
            "import {{ defineStore }} from 'pinia';\n\
             export const useStore = defineStore('s', () => {{\n{body}\treturn {{ {} }};\n}});\n",
            keys.join(", ")
        );
        let view = format!(
            "import {{ useStore }} from './stores';\n\nexport function view() {{\n\tconst store = useStore();\n{calls}}}\n"
        );
        let started = Stopwatch::start();
        let found = rows(&[("stores.ts", &stores), ("view.ts", &view)], "view.ts");
        let elapsed = started.elapsed();
        assert_eq!(found.len(), size + 1);
        let last = size - 1;
        assert_eq!(
            found[size],
            format!(
                "{} view>store.run{last} UniqueCandidate ReceiverDeclaredType [stores.ts#run{last}]",
                5 + last
            )
        );
        elapsed
    }

    #[test]
    fn test_timing_cases_resolve_each_call() {
        stores_in_one_file_at(3);
        members_of_one_store_at(3);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_stores_in_one_file_resolve_in_linear_time() {
        assert_scales_linearly("stores in one file", 4_000, stores_in_one_file_at);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_members_of_one_store_resolve_in_linear_time() {
        assert_scales_linearly("members of one store", 8_000, members_of_one_store_at);
    }
}
