//! Python calls into a base class outside the repository.
//!
//! Two rules give `External` with evidence from the source:
//!
//! - `super().name()` in a method of class `C` finds `name` in the class
//!   dictionaries of the method resolution order after `C`. When no class of
//!   the repository on that order binds `name`, the method is in a class
//!   outside the repository.
//! - `Model.objects.name()` on a Django model: Django adds `objects`, an
//!   object of `django.db.models.Manager`, when no class of the model binds
//!   it. With a manager of a different name, `objects` does not exist.
//!
//! A class of the repository whose body, decorators, metaclass, or
//! `__getattr__` can add attributes that the source does not show makes the
//! rules abstain. So does a write of the attribute on any object other than
//! `self`.

use std::collections::{HashSet, VecDeque};

use super::{segments, Family, Id, Resolver};
use crate::index::lang::python_classes::{ClassBody, ANY_ATTRIBUTE};
use crate::index::model::{ImportKind, Receiver, ReferenceKind, SymbolKind};

/// The number of classes of the repository that an ancestry can have. A
/// larger ancestry makes the rules abstain, so that a lookup has a fixed cost.
const MAXIMUM_ANCESTORS: usize = 64;

/// The full names of the Django model base class.
const DJANGO_MODELS: &[&str] = &["django.db.models.Model", "django.db.models.base.Model"];

/// Names that let a class of the repository add attributes to a class or
/// read them in a way that the class body does not show.
const DYNAMIC_MEMBERS: &[&str] = &["__getattr__", "__getattribute__", "__init_subclass__"];

/// What a base class expression of a Python class names.
enum Base {
    Repository(Id),
    /// A class outside the repository, by its full dotted name.
    External(String),
    Unknown,
}

/// The classes of the repository that a class has as itself or as an
/// ancestor, and the full names of its ancestors outside the repository.
struct Ancestry {
    repository: Vec<Id>,
    external: Vec<String>,
}

impl<'a> Resolver<'a> {
    /// The call at `position` is a Python call of a member of a base class
    /// outside the repository.
    pub(super) fn is_call_into_external_base(&self, file: usize, position: usize) -> bool {
        if self.family(file) != Family::Python || self.has_any_attribute_write() {
            return false;
        }
        let facts = &self.files[file];
        let reference = &facts.references[position];
        if reference.kind != ReferenceKind::Call {
            return false;
        }
        let site = &facts.reference_facts[position];
        let chain = self.chain(file, site.owner);
        match &reference.receiver {
            Some(Receiver::Super) => self.is_external_super_call(&chain, site, &reference.name),
            Some(Receiver::SelfValue | Receiver::Expression) => false,
            Some(Receiver::Name(_)) | None => match site.receiver_path.as_deref() {
                Some([model, objects]) if objects == "objects" => {
                    self.is_default_manager(file, &chain, site.start_byte, model)
                }
                _ => false,
            },
        }
    }

    fn is_external_super_call(
        &self,
        chain: &[Id],
        site: &crate::index::extract::ReferenceFacts,
        name: &str,
    ) -> bool {
        // Only `super()` without arguments searches after the class that
        // has the method.
        let bare = site
            .receiver
            .is_some_and(|(start, end)| end - start == "super()".len());
        let Some(class) = self.enclosing_type(chain) else {
            return false;
        };
        let Some(method) = chain.first() else {
            return false;
        };
        let direct = method.0 == class.0 && self.facts(*method).parent == Some(class.1);
        if !bare || !direct || self.is_followed_in_python(class) || self.is_attribute_written(name)
        {
            return false;
        }
        let Some(ancestry) = self.python_ancestry(class) else {
            return false;
        };
        !ancestry.external.is_empty()
            && ancestry.repository[1..].iter().all(|id| {
                self.class_body(*id)
                    .is_some_and(|body| !body.names.contains(name))
            })
    }

    fn is_default_manager(&self, file: usize, chain: &[Id], at: usize, model: &str) -> bool {
        if self.local(chain, file, model, Some(at)).is_some()
            || self.is_attribute_written("objects")
        {
            return false;
        }
        let Some(class) = self.python_class_named(file, model) else {
            return false;
        };
        let Some(ancestry) = self.python_ancestry(class) else {
            return false;
        };
        ancestry
            .external
            .iter()
            .any(|base| DJANGO_MODELS.contains(&base.as_str()))
            && ancestry.repository.iter().all(|id| {
                self.class_body(*id)
                    .is_some_and(|body| !body.names.contains("objects"))
            })
    }

    /// The class of the repository that a name in `file` gives: a class of the
    /// module, or a class that an import binds.
    fn python_class_named(&self, file: usize, text: &str) -> Option<Id> {
        match self.python_base(file, text) {
            Base::Repository(id) => Some(id),
            _ => None,
        }
    }

    fn class_body(&self, id: Id) -> Option<&'a ClassBody> {
        let bodies = &self.files[id.0].python_classes;
        let found = bodies
            .binary_search_by_key(&id.1, |body| body.symbol)
            .ok()?;
        Some(&bodies[found])
    }

    /// What the base class expression `text` of a class in `file` names.
    fn python_base(&self, file: usize, text: &str) -> Base {
        // A generic base, as `Generic[T]` is, names the class before `[`.
        let text = text.split('[').next().unwrap_or_default().trim();
        let path = segments(text);
        let Some((&head, rest)) = path.split_first() else {
            return Base::Unknown;
        };
        if path.iter().any(|segment| !is_identifier(segment)) {
            return Base::Unknown;
        }
        let Some((&name, qualifier)) = path.split_last() else {
            return Base::Unknown;
        };
        let qualifier = (!qualifier.is_empty()).then(|| qualifier.join("."));
        if let Some(binding) = self.binding_of(file, head) {
            if self.is_external_binding(binding) {
                return Base::External(external_name(binding, &path, rest));
            }
            return match self.types(name, qualifier.as_deref(), file).as_slice() {
                [only] if self.symbol(*only).kind == SymbolKind::Class => Base::Repository(*only),
                _ => Base::Unknown,
            };
        }
        let defined: Vec<Id> = self
            .named_in(head, file)
            .filter(|id| self.symbol(*id).container.is_none())
            .collect();
        match (defined.as_slice(), rest) {
            ([only], []) if self.symbol(*only).kind == SymbolKind::Class => Base::Repository(*only),
            ([], [])
                if !self.has_repository_wildcard(file)
                    && super::external::is_python_builtin_class(head) =>
            {
                Base::External(format!("builtins.{head}"))
            }
            _ => Base::Unknown,
        }
    }

    /// The ancestry of a Python class, with the class first. `None` when a
    /// base is not known, when a class of the repository can add attributes
    /// that the source does not show, or when the ancestry is too large.
    fn python_ancestry(&self, class: Id) -> Option<Ancestry> {
        let mut ancestry = Ancestry {
            repository: Vec::new(),
            external: Vec::new(),
        };
        let mut seen = HashSet::from([class]);
        let mut pending = VecDeque::from([class]);
        while let Some(id) = pending.pop_front() {
            if ancestry.repository.len() == MAXIMUM_ANCESTORS || self.family(id.0) != Family::Python
            {
                return None;
            }
            let body = self.class_body(id)?;
            if body.opaque
                || DYNAMIC_MEMBERS
                    .iter()
                    .any(|name| body.names.contains(*name))
            {
                return None;
            }
            ancestry.repository.push(id);
            for text in &self.symbol(id).supertypes {
                match self.python_base(id.0, text) {
                    Base::Repository(parent) => {
                        if seen.insert(parent) {
                            pending.push_back(parent);
                        }
                    }
                    Base::External(full) => ancestry.external.push(full),
                    Base::Unknown => return None,
                }
            }
        }
        Some(ancestry)
    }

    /// A class of the repository can follow `class` in the method resolution
    /// order of a subclass. In the order of a class with the bases `A, B`,
    /// `B` follows `A` and each ancestor of `A` that is not an ancestor of
    /// `B`. The ancestors of a base before `A` are before `A`.
    fn is_followed_in_python(&self, class: Id) -> bool {
        self.python_followed
            .get_or_init(|| {
                let mut followed = HashSet::new();
                for (file, facts) in self.files.iter().enumerate() {
                    for body in &facts.python_classes {
                        let bases: Vec<Base> = self
                            .symbol((file, body.symbol))
                            .supertypes
                            .iter()
                            .map(|text| self.python_base(file, text))
                            .collect();
                        let Some(last) = bases
                            .iter()
                            .rposition(|base| !matches!(base, Base::External(_)))
                        else {
                            continue;
                        };
                        let mut pending: Vec<Id> = bases[..last]
                            .iter()
                            .filter_map(|base| match base {
                                Base::Repository(parent) => Some(*parent),
                                _ => None,
                            })
                            .collect();
                        while let Some(parent) = pending.pop() {
                            if !followed.insert(parent) {
                                continue;
                            }
                            for text in &self.symbol(parent).supertypes {
                                if let Base::Repository(next) = self.python_base(parent.0, text) {
                                    pending.push(next);
                                }
                            }
                        }
                    }
                }
                followed
            })
            .contains(&class)
    }

    fn is_attribute_written(&self, name: &str) -> bool {
        self.python_attribute_writes().contains(name)
    }

    fn has_any_attribute_write(&self) -> bool {
        self.python_attribute_writes().contains(ANY_ATTRIBUTE)
    }

    fn python_attribute_writes(&self) -> &HashSet<&'a str> {
        self.python_writes.get_or_init(|| {
            self.files
                .iter()
                .filter(|facts| facts.language == crate::core::Language::Python)
                .flat_map(|facts| facts.attribute_writes.iter().map(String::as_str))
                .collect()
        })
    }
}

/// The full dotted name of a base class whose first name an import from
/// outside the repository binds.
fn external_name(
    binding: &crate::index::model::ImportBinding,
    path: &[&str],
    rest: &[&str],
) -> String {
    let mut full = match binding.kind {
        ImportKind::Named => {
            let imported = binding.imported_name.as_deref().unwrap_or_default();
            format!("{}.{imported}", binding.module_path)
        }
        // `import a.b` binds `a`, and the path of the base is the full name.
        // `import a.b as c` binds `c` to `a.b`.
        _ if binding.local_name.as_deref() == binding.module_path.split('.').next() => {
            return path.join(".");
        }
        _ => binding.module_path.clone(),
    };
    for segment in rest {
        full.push('.');
        full.push_str(segment);
    }
    full
}

fn is_identifier(text: &str) -> bool {
    let mut characters = text.chars();
    characters
        .next()
        .is_some_and(|first| first == '_' || first.is_alphabetic())
        && characters.all(|character| character == '_' || character.is_alphanumeric())
}

#[cfg(test)]
mod tests {
    use crate::index::resolve::testing::rows;

    /// The status of each call in `file` whose callee text is `callee`.
    fn statuses(files: &[(&str, &str)], file: &str, callee: &str) -> Vec<String> {
        let found: Vec<String> = rows(files, file)
            .into_iter()
            .filter(|row| {
                let text = row.split(' ').nth(1).unwrap_or_default();
                text.rsplit('>').next() == Some(callee)
            })
            .map(|row| row.split(' ').nth(2).unwrap_or_default().to_string())
            .collect();
        assert!(
            !found.is_empty(),
            "no call {callee}: {:?}",
            rows(files, file)
        );
        found
    }

    fn external(files: &[(&str, &str)], file: &str, callee: &str) -> bool {
        statuses(files, file, callee)
            .iter()
            .all(|status| status == "External")
    }

    const COMMAND: &str = "from django.core.management.base import BaseCommand\n\n\nclass Command(BaseCommand):\n    def __init__(self, *args):\n        super().__init__(*args)\n\n    def handle(self):\n        super().handle()\n";

    #[test]
    fn test_super_call_into_an_external_base_is_external() {
        let files = [("command.py", COMMAND)];
        assert!(external(&files, "command.py", "super.__init__"));
        assert!(external(&files, "command.py", "super.handle"));
    }

    #[test]
    fn test_super_call_through_repository_bases() {
        let base = "from django.core.management.base import BaseCommand\n\n\nclass Base(BaseCommand):\n    def check(self):\n        pass\n";
        let child = "from base import Base\n\n\nclass Child(Base):\n    def __init__(self):\n        super().__init__()\n\n    def check(self):\n        super().check()\n";
        let files = [("base.py", base), ("child.py", child)];
        assert!(external(&files, "child.py", "super.__init__"));
        assert!(!external(&files, "child.py", "super.check"));
    }

    #[test]
    fn test_super_call_abstains_on_each_guard() {
        let cases = [
            // An attribute of a repository base that is not a definition.
            "from ext import Base\n\n\nclass Mid(Base):\n    run = make()\n\n\nclass C(Mid):\n    def run(self):\n        super().run()\n",
            // `__getattr__` of a base of the repository.
            "from ext import Base\n\n\nclass Mid(Base):\n    def __getattr__(self, name):\n        return 1\n\n\nclass C(Mid):\n    def run(self):\n        super().run()\n",
            // A metaclass.
            "from ext import Base\n\n\nclass Meta(type):\n    pass\n\n\nclass C(Base, metaclass=Meta):\n    def run(self):\n        super().run()\n",
            // A decorator.
            "from ext import Base\nfrom deco import add\n\n\n@add\nclass C(Base):\n    def run(self):\n        super().run()\n",
            // A base that the source does not show.
            "from ext import Base\n\n\nclass C(Base, make()):\n    def run(self):\n        super().run()\n",
            "class C(Unknown):\n    def run(self):\n        super().run()\n",
            // A base of the repository whose own base is not known.
            "from ext import Base\n\n\nclass Mid(Unknown):\n    pass\n\n\nclass C(Base, Mid):\n    def run(self):\n        super().run()\n",
            // A repository class can follow C in the order of a subclass.
            "from ext import Base\n\n\nclass Mixin:\n    def run(self):\n        pass\n\n\nclass C(Base):\n    def run(self):\n        super().run()\n\n\nclass D(C, Mixin):\n    pass\n",
            // A write of the attribute on a class.
            "from ext import Base\n\n\nclass C(Base):\n    def run(self):\n        super().run()\n\n\nBase.run = print\n",
            "from ext import Base\n\n\nclass C(Base):\n    def run(self):\n        super().run()\n\n\nsetattr(Base, name, print)\n",
            // `super` with arguments names a different start of the search.
            "from ext import Base\n\n\nclass C(Base):\n    def run(self):\n        super(Base, self).run()\n",
        ];
        for source in cases {
            let files = [("app.py", source)];
            assert!(!external(&files, "app.py", "super.run"), "{source}");
        }
    }

    #[test]
    fn test_super_call_with_only_external_bases_after_it_is_external() {
        // An external class after C in the order of D, and a repository
        // class before C, do not change the method that C finds.
        let source = "from ext import Base, Other\n\n\nclass First:\n    def run(self):\n        pass\n\n\nclass C(Base):\n    def run(self):\n        super().run()\n\n\nclass D(C, Other):\n    pass\n\n\nclass E(First, C):\n    pass\n";
        let files = [("app.py", source)];
        assert!(external(&files, "app.py", "super.run"));
    }

    const MODELS: &str = "from django.db import models\n\n\nclass Product(models.Model):\n    name = models.CharField()\n";

    #[test]
    fn test_default_manager_of_a_django_model_is_external() {
        let app = "from models import Product\n\n\ndef run():\n    return Product.objects.filter(name=1)\n";
        let files = [("models.py", MODELS), ("app.py", app)];
        assert!(external(&files, "app.py", "Product.objects::filter"));
        let same = format!("{MODELS}\n\ndef run():\n    return Product.objects.all()\n");
        let files = [("models.py", same.as_str())];
        assert!(external(&files, "models.py", "Product.objects::all"));
        let model = "from django.db.models import Model\n\n\nclass Product(Model):\n    pass\n\n\ndef run():\n    return Product.objects.all()\n";
        assert!(external(
            &[("models.py", model)],
            "models.py",
            "Product.objects::all"
        ));
    }

    #[test]
    fn test_default_manager_abstains_on_each_guard() {
        let call = "\n\ndef run():\n    return Product.objects.filter(name=1)\n";
        let cases = [
            // A manager of the model.
            "from django.db import models\n\n\nclass Product(models.Model):\n    objects = ProductManager()\n",
            // A manager of an abstract base.
            "from django.db import models\n\n\nclass Base(models.Model):\n    objects = BaseManager()\n\n    class Meta:\n        abstract = True\n\n\nclass Product(Base):\n    pass\n",
            // A model that is not a Django model.
            "from other.db import models\n\n\nclass Product(models.Model):\n    pass\n",
            // A repository module in place of Django.
            "from shim import models\n\n\nclass Product(models.Model):\n    pass\n",
            // A decorator, a metaclass, and a write of the attribute.
            "from django.db import models\nfrom deco import add\n\n\n@add\nclass Product(models.Model):\n    pass\n",
            "from django.db import models\n\n\nclass Meta(type):\n    pass\n\n\nclass Product(models.Model, metaclass=Meta):\n    pass\n",
            "from django.db import models\n\n\nclass Product(models.Model):\n    pass\n\n\nProduct.objects = ProductManager()\n",
            // A base that the source does not show.
            "from django.db import models\n\n\nclass Product(models.Model, Unknown):\n    pass\n",
        ];
        let shim = ("shim.py", "class models:\n    class Model:\n        pass\n");
        for models in cases {
            let source = format!("{models}{call}");
            let files = [("models.py", source.as_str()), shim];
            assert!(
                !external(&files, "models.py", "Product.objects::filter"),
                "{models}"
            );
        }
    }

    /// A manager under a different name: Django makes no `objects`, thus the
    /// call fails and is not a call of the repository either.
    #[test]
    fn test_default_manager_with_a_manager_of_a_different_name_is_external() {
        let source = "from django.db import models\n\n\nclass Product(models.Model):\n    items = models.Manager()\n\n\ndef run():\n    return Product.objects.filter(name=1)\n";
        assert!(external(
            &[("models.py", source)],
            "models.py",
            "Product.objects::filter"
        ));
    }

    #[test]
    fn test_local_name_is_not_the_model() {
        let source =
            format!("{MODELS}\n\ndef run(Product):\n    return Product.objects.filter(name=1)\n");
        assert!(!external(
            &[("models.py", source.as_str())],
            "models.py",
            "Product.objects::filter"
        ));
    }

    /// `size` classes with a `super()` call into an external base, a chain of
    /// `size` classes, a subclass with a mixin for each class of the chain,
    /// and `size` calls of a default manager.
    fn calls_into_external_bases_at(size: usize) -> std::time::Duration {
        let mut source = String::from(
            "from ext import Base\nfrom django.db import models\n\n\nclass Mixin:\n    pass\n\n\nclass Product(models.Model):\n    pass\n\n\nclass D0(Base):\n    pass\n",
        );
        for index in 0..size {
            source.push_str(&format!(
                "\n\nclass C{index}(Base):\n    def run(self):\n        return super().run()\n"
            ));
            let parent = index.saturating_sub(1);
            source.push_str(&format!(
                "\n\nclass D{}(D{parent}):\n    def run(self):\n        return super().run()\n\n\nclass M{index}(D{index}, Mixin):\n    pass\n",
                index + 1
            ));
            source.push_str(&format!(
                "\n\ndef get{index}():\n    return Product.objects.filter(name={index})\n"
            ));
        }
        let files = [("app.py", source.as_str())];
        let started = crate::index::timing::Stopwatch::start();
        let (mut facts, manifests) = crate::index::imports::testing::index_with_manifests(&files);
        crate::index::resolve::resolve_references_with(&mut facts, &manifests);
        let elapsed = started.elapsed();
        let external = facts
            .iter()
            .flat_map(|file| &file.references)
            .filter(|reference| {
                ["run", "filter"].contains(&reference.name.as_str())
                    && reference.resolution.status
                        == crate::index::model::ResolutionStatus::External
            })
            .count();
        assert_eq!(external, 2 * size);
        elapsed
    }

    #[test]
    fn test_each_call_into_an_external_base_is_external() {
        calls_into_external_bases_at(20);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_calls_into_external_bases_resolve_in_linear_time() {
        crate::index::timing::assert_scales_linearly("bases", 1_000, calls_into_external_bases_at);
    }
}
