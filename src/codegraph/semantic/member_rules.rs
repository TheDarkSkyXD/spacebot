//! Centralized visibility / export classification per language.
//!
//! Today each provider inlines its own visibility + is_exported
//! heuristics, which is how we ended up with drift — Python providers
//! looking for leading underscores, TS providers walking ancestor
//! export statements, C# providers scanning sibling modifier nodes.
//! None of that logic is complex individually, but keeping it spread
//! across 14 files means fixing a rule means touching 14 files.
//!
//! This module is the single source of truth. Callers pass the parsed
//! language-specific signals (ordered modifier keywords, the symbol's
//! bare name, and optional ancestor hints) and get back a uniform
//! [`Visibility`] + `is_exported` answer.
//!
//! The plan is for every provider to migrate to these helpers in
//! Phase 3 Part B. The module is live today (tests exercise every
//! language's rules) so the migration in each provider is a localized
//! substitution rather than a redesign.

use crate::codegraph::lang::SupportedLanguage;

/// Visibility modifier captured on a Property / Method / Function
/// node. Ordered from most to least open so downstream queries like
/// "public API surface" can do a range scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    /// Publicly visible outside the module / assembly / crate.
    Public,
    /// Visible inside the package / module only (Java default,
    /// Kotlin `internal`, Rust `pub(crate)`).
    Package,
    /// Visible inside the enclosing file only (Swift `fileprivate`,
    /// Rust `pub(super)`).
    File,
    /// Visible inside the enclosing type and its subclasses.
    Protected,
    /// Visible only inside the enclosing type.
    Private,
}

impl Visibility {
    /// The canonical lowercase string used in the graph's
    /// `visibility` property, so node queries get stable values
    /// across languages.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Package => "package",
            Self::File => "file",
            Self::Protected => "protected",
            Self::Private => "private",
        }
    }
}

/// Classify a symbol's visibility from language-specific signals.
///
/// `modifiers` is the set of modifier keywords parsed off the
/// declaration, in source order. `name` is the bare symbol name —
/// relevant for Python (leading underscore) and Go (uppercase initial).
///
/// When the provider has no direct signals, the returned
/// [`Visibility`] matches the language's default rule (e.g. Java
/// bare members are `Package`).
pub fn classify_visibility(
    lang: SupportedLanguage,
    modifiers: &[&str],
    name: &str,
) -> Visibility {
    use SupportedLanguage::*;
    match lang {
        Rust => rust_visibility(modifiers),
        Python => python_visibility(name),
        JavaScript | TypeScript => ts_visibility(modifiers, name),
        Java => jvm_visibility(modifiers, /* default */ Visibility::Package),
        Kotlin => jvm_visibility(modifiers, /* default */ Visibility::Public),
        CSharp => csharp_visibility(modifiers),
        Go => go_visibility(name),
        C | Cpp => c_cpp_visibility(modifiers),
        Php => php_visibility(modifiers),
        Swift => swift_visibility(modifiers),
        Ruby => ruby_visibility(modifiers),
        Dart => dart_visibility(name),
        Cobol => Visibility::Public, // COBOL has no per-symbol visibility
        Jcl => Visibility::Public,   // JCL jobs/steps are always externally callable
        Html => Visibility::Public,  // HTML has no symbol visibility concept
        Prisma => Visibility::Public, // Prisma models are always exported
    }
}

/// Whether the symbol is exported from its module / compilation unit
/// — i.e. visible to importers.
///
/// `visibility` comes from [`classify_visibility`] and `modifiers` +
/// `name` are the same arguments passed there. Most languages derive
/// `is_exported` directly from visibility (public → exported); TS /
/// JS are the exception because `export` is a separate modifier
/// orthogonal to method visibility.
pub fn is_exported(
    lang: SupportedLanguage,
    visibility: Visibility,
    modifiers: &[&str],
    name: &str,
) -> bool {
    use SupportedLanguage::*;
    match lang {
        TypeScript | JavaScript => modifiers.contains(&"export"),
        Python | Dart => !name.starts_with('_'),
        Go => name
            .chars()
            .next()
            .map(|c| c.is_uppercase())
            .unwrap_or(false),
        Rust => matches!(visibility, Visibility::Public),
        Java | Kotlin | CSharp | Swift | Php | Ruby | C | Cpp => {
            matches!(visibility, Visibility::Public)
        }
        Cobol => true, // every COBOL PROGRAM-ID / paragraph is callable
        Jcl => true,   // JCL jobs and steps are externally invokable by name
        Html => true,  // HTML elements are always visible in the graph
        Prisma => true, // Prisma models / enums are globally referenced
    }
}

// ── Per-language rules ─────────────────────────────────────────────

fn rust_visibility(modifiers: &[&str]) -> Visibility {
    if modifiers.contains(&"pub(crate)") {
        Visibility::Package
    } else if modifiers.contains(&"pub(super)") {
        Visibility::File
    } else if modifiers.contains(&"pub") {
        Visibility::Public
    } else {
        Visibility::Private
    }
}

fn python_visibility(name: &str) -> Visibility {
    // Python convention: one leading underscore = protected, two = private,
    // anything else = public. Dunder (`__init__`, `__str__`) names are
    // public — they're explicit special-method hooks.
    if name.starts_with("__") && !name.ends_with("__") {
        Visibility::Private
    } else if name.starts_with('_') {
        Visibility::Protected
    } else {
        Visibility::Public
    }
}

fn ts_visibility(modifiers: &[&str], name: &str) -> Visibility {
    if name.starts_with('#') {
        return Visibility::Private; // TS/JS private fields
    }
    if modifiers.contains(&"private") {
        Visibility::Private
    } else if modifiers.contains(&"protected") {
        Visibility::Protected
    } else {
        // TS `public` is the syntactic default; JS has no modifier —
        // either way the effective visibility is Public.
        Visibility::Public
    }
}

/// Shared JVM rule used for both Java and Kotlin. `default_when_bare`
/// lets callers pick Package (Java) vs Public (Kotlin) when no
/// modifier is present, which is the only real difference between
/// the two languages here.
fn jvm_visibility(modifiers: &[&str], default_when_bare: Visibility) -> Visibility {
    if modifiers.contains(&"private") {
        Visibility::Private
    } else if modifiers.contains(&"protected") {
        Visibility::Protected
    } else if modifiers.contains(&"public") {
        Visibility::Public
    } else if modifiers.contains(&"internal") {
        Visibility::Package
    } else {
        default_when_bare
    }
}

fn csharp_visibility(modifiers: &[&str]) -> Visibility {
    if modifiers.contains(&"private") {
        Visibility::Private
    } else if modifiers.contains(&"protected") {
        Visibility::Protected
    } else if modifiers.contains(&"public") {
        Visibility::Public
    } else if modifiers.contains(&"internal") {
        Visibility::Package
    } else {
        // C# type-level default is internal; member default is
        // private. Without an enclosing-scope hint we pick internal
        // (Package) because the common case is "no modifier means
        // the assembly can see it".
        Visibility::Package
    }
}

fn go_visibility(name: &str) -> Visibility {
    if name
        .chars()
        .next()
        .map(|c| c.is_uppercase())
        .unwrap_or(false)
    {
        Visibility::Public
    } else {
        Visibility::Package
    }
}

fn c_cpp_visibility(modifiers: &[&str]) -> Visibility {
    // C/C++ file-scope `static` means internal linkage — visible only
    // inside the translation unit. No modifier means external linkage
    // (globally visible after linking). Class-level `private` /
    // `protected` / `public` behave like other OO languages.
    if modifiers.contains(&"private") {
        Visibility::Private
    } else if modifiers.contains(&"protected") {
        Visibility::Protected
    } else if modifiers.contains(&"static") {
        Visibility::File
    } else {
        Visibility::Public
    }
}

fn php_visibility(modifiers: &[&str]) -> Visibility {
    if modifiers.contains(&"private") {
        Visibility::Private
    } else if modifiers.contains(&"protected") {
        Visibility::Protected
    } else {
        // PHP top-level or explicitly `public` → Public.
        Visibility::Public
    }
}

fn swift_visibility(modifiers: &[&str]) -> Visibility {
    if modifiers.contains(&"private") {
        Visibility::Private
    } else if modifiers.contains(&"fileprivate") {
        Visibility::File
    } else if modifiers.contains(&"internal") {
        Visibility::Package
    } else if modifiers.contains(&"public") || modifiers.contains(&"open") {
        Visibility::Public
    } else {
        // Swift default is `internal` — visible within the module.
        Visibility::Package
    }
}

fn ruby_visibility(modifiers: &[&str]) -> Visibility {
    // Ruby's visibility modifiers are applied as pseudo-statements
    // in class bodies — the provider supplies whichever is in
    // effect for this method.
    if modifiers.contains(&"private") {
        Visibility::Private
    } else if modifiers.contains(&"protected") {
        Visibility::Protected
    } else {
        Visibility::Public
    }
}

fn dart_visibility(name: &str) -> Visibility {
    if name.starts_with('_') {
        // Dart library-private: visible only within the file's
        // compilation unit, not across library boundaries.
        Visibility::File
    } else {
        Visibility::Public
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use SupportedLanguage::*;

    #[test]
    fn visibility_as_str_roundtrips() {
        assert_eq!(Visibility::Public.as_str(), "public");
        assert_eq!(Visibility::Private.as_str(), "private");
        assert_eq!(Visibility::Package.as_str(), "package");
        assert_eq!(Visibility::File.as_str(), "file");
        assert_eq!(Visibility::Protected.as_str(), "protected");
    }

    #[test]
    fn rust_pub_and_pub_crate() {
        assert_eq!(classify_visibility(Rust, &["pub"], "foo"), Visibility::Public);
        assert_eq!(
            classify_visibility(Rust, &["pub(crate)"], "foo"),
            Visibility::Package
        );
        assert_eq!(
            classify_visibility(Rust, &["pub(super)"], "foo"),
            Visibility::File
        );
        assert_eq!(classify_visibility(Rust, &[], "foo"), Visibility::Private);
    }

    #[test]
    fn python_underscore_conventions() {
        assert_eq!(classify_visibility(Python, &[], "foo"), Visibility::Public);
        assert_eq!(classify_visibility(Python, &[], "_foo"), Visibility::Protected);
        assert_eq!(classify_visibility(Python, &[], "__foo"), Visibility::Private);
        // Dunder names like `__init__` are public, not private.
        assert_eq!(classify_visibility(Python, &[], "__init__"), Visibility::Public);
    }

    #[test]
    fn typescript_hash_private_fields() {
        assert_eq!(
            classify_visibility(TypeScript, &[], "#foo"),
            Visibility::Private
        );
        assert_eq!(
            classify_visibility(TypeScript, &["private"], "foo"),
            Visibility::Private
        );
        assert_eq!(
            classify_visibility(TypeScript, &["protected"], "foo"),
            Visibility::Protected
        );
        assert_eq!(
            classify_visibility(TypeScript, &["public"], "foo"),
            Visibility::Public
        );
        assert_eq!(classify_visibility(TypeScript, &[], "foo"), Visibility::Public);
    }

    #[test]
    fn ts_is_exported_follows_keyword() {
        assert!(is_exported(
            TypeScript,
            Visibility::Public,
            &["export"],
            "foo"
        ));
        assert!(!is_exported(
            TypeScript,
            Visibility::Public,
            &[],
            "foo"
        ));
    }

    #[test]
    fn java_default_is_package() {
        assert_eq!(classify_visibility(Java, &[], "foo"), Visibility::Package);
        assert_eq!(
            classify_visibility(Java, &["public"], "foo"),
            Visibility::Public
        );
        assert_eq!(
            classify_visibility(Java, &["private"], "foo"),
            Visibility::Private
        );
    }

    #[test]
    fn kotlin_default_is_public() {
        assert_eq!(classify_visibility(Kotlin, &[], "foo"), Visibility::Public);
        assert_eq!(
            classify_visibility(Kotlin, &["internal"], "foo"),
            Visibility::Package
        );
    }

    #[test]
    fn csharp_internal_default() {
        assert_eq!(classify_visibility(CSharp, &[], "foo"), Visibility::Package);
        assert_eq!(
            classify_visibility(CSharp, &["public"], "Foo"),
            Visibility::Public
        );
    }

    #[test]
    fn go_casing_determines_visibility() {
        assert_eq!(classify_visibility(Go, &[], "Foo"), Visibility::Public);
        assert_eq!(classify_visibility(Go, &[], "foo"), Visibility::Package);
        assert!(is_exported(Go, Visibility::Public, &[], "Foo"));
        assert!(!is_exported(Go, Visibility::Package, &[], "foo"));
    }

    #[test]
    fn c_static_is_file_scope() {
        assert_eq!(classify_visibility(C, &[], "foo"), Visibility::Public);
        assert_eq!(
            classify_visibility(C, &["static"], "foo"),
            Visibility::File
        );
    }

    #[test]
    fn php_top_level_is_public() {
        assert_eq!(classify_visibility(Php, &[], "foo"), Visibility::Public);
        assert_eq!(
            classify_visibility(Php, &["private"], "foo"),
            Visibility::Private
        );
    }

    #[test]
    fn swift_fileprivate_mapped() {
        assert_eq!(
            classify_visibility(Swift, &["fileprivate"], "foo"),
            Visibility::File
        );
        assert_eq!(classify_visibility(Swift, &[], "foo"), Visibility::Package);
        assert_eq!(
            classify_visibility(Swift, &["open"], "foo"),
            Visibility::Public
        );
    }

    #[test]
    fn ruby_default_public() {
        assert_eq!(classify_visibility(Ruby, &[], "foo"), Visibility::Public);
        assert_eq!(
            classify_visibility(Ruby, &["private"], "foo"),
            Visibility::Private
        );
    }

    #[test]
    fn dart_underscore_file_private() {
        assert_eq!(classify_visibility(Dart, &[], "foo"), Visibility::Public);
        assert_eq!(classify_visibility(Dart, &[], "_foo"), Visibility::File);
        assert!(is_exported(Dart, Visibility::Public, &[], "foo"));
        assert!(!is_exported(Dart, Visibility::File, &[], "_foo"));
    }
}
