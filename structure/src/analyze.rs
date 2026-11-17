//! Reads a crate's source and says, for each function, what it calls and
//! which of the constructs this tool is about it contains.
//!
//! Everything here is syntax. Nothing is type-checked, so a call is matched
//! to functions by name, as described on [`Crate::reachable`]. Where a call
//! cannot be followed at all, that is recorded and ends up in the report.

use proc_macro2::Span;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use syn::visit::{self, Visit};

/// Method calls that make an owned copy or a new collection (check S3).
pub const COPYING_METHODS: [&str; 6] = [
    "clone",
    "cloned",
    "collect",
    "to_vec",
    "to_owned",
    "to_string",
];
/// Return types that are a collection the caller owns. A function that
/// returns one and contains a copying construct makes its copy at every
/// call (reported beside S3).
const OWNED_COLLECTIONS: [&str; 7] = [
    "Vec", "VecDeque", "String", "HashMap", "HashSet", "BTreeMap", "BTreeSet",
];
/// Macros of the standard library that allocate. Reported beside S3.
const ALLOCATING_MACROS: [&str; 2] = ["format", "vec"];
/// Standard macros whose arguments are ordinary expressions and are read as
/// such. Any other macro is reported as not expanded.
const KNOWN_MACROS: [&str; 22] = [
    "format",
    "vec",
    "println",
    "eprintln",
    "print",
    "eprint",
    "write",
    "writeln",
    "assert",
    "assert_eq",
    "assert_ne",
    "debug_assert",
    "debug_assert_eq",
    "debug_assert_ne",
    "panic",
    "unreachable",
    "todo",
    "unimplemented",
    "matches",
    "dbg",
    "format_args",
    "thread_local",
];

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Hit {
    pub what: String,
    pub file: String,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub enum Call {
    /// `f(..)`, `module::f(..)`, `Type::f(..)`: the path as written, and
    /// the line.
    Path(Vec<String>, usize),
    /// `x.f(..)`; `on_self` when the receiver is exactly `self`.
    Method { name: String, on_self: bool },
}

/// A field of `self` taken out with `mem::take`, `mem::replace` or
/// `Option::take`.
#[derive(Debug, Clone)]
pub struct Take {
    pub field: String,
    pub how: String,
    pub line: usize,
}

#[derive(Debug, Default)]
pub struct Function {
    /// `module::Type::name` or `module::name`.
    pub name: String,
    pub short: String,
    pub self_ty: Option<String>,
    /// The module the function is written in.
    pub module: Vec<String>,
    /// For a method written in `impl Trait for Type`: the trait's path as
    /// written there.
    pub impl_trait: Option<Vec<String>>,
    /// For a method: the type's path as written in the `impl` header, and
    /// a placeholder for each generic argument written after it.
    pub self_written: Vec<String>,
    pub self_args: Vec<&'static str>,
    /// How a method takes `self`: `exclusive` (`&mut self`), `shared`
    /// (`&self`) or `value`. `None` for a function with no `self`.
    pub receiver: Option<&'static str>,
    /// The return type as written is an owned collection or a `String`.
    pub returns_collection: bool,
    pub file: String,
    pub line: usize,
    pub lines: usize,
    pub calls: Vec<Call>,
    pub copying: Vec<Hit>,
    pub allocating_macros: Vec<Hit>,
    pub takes: Vec<Take>,
    pub assigned_self_fields: BTreeSet<String>,
    pub threads: Vec<Hit>,
    pub not_followed: Vec<Hit>,
}

/// `Rc<RefCell<..>>`, `Arc<Mutex<..>>` or `Weak<..>`, and where (check S7).
#[derive(Debug, Clone, Serialize)]
pub struct Shared {
    pub construct: String,
    /// The struct or function it appears in.
    #[serde(rename = "in")]
    pub item: String,
    pub file: String,
    pub line: usize,
}

#[derive(Debug, Default)]
pub struct Crate {
    pub functions: Vec<Function>,
    /// What is shared by its spelling alone. With the rest:
    /// [`Crate::shared_ownership`].
    pub shared: Vec<Shared>,
    /// `Rc<T>` and `Arc<T>` as written, by the name of `T`: shared
    /// ownership if `T` turns out to be in `cell_structs`.
    shared_by_type: Vec<(String, Shared)>,
    /// Structs of the crate with a field whose type names `Cell`,
    /// `RefCell`, `Mutex`, `RwLock` or an atomic.
    cell_structs: BTreeSet<String>,
    pub unsafe_uses: Vec<Hit>,
    /// `forbid` or `deny` when the crate root says so for `unsafe_code`.
    pub unsafe_lint: Option<String>,
    pub file_lines: BTreeMap<String, usize>,
    pub impl_blocks: BTreeMap<String, usize>,
    pub struct_fields: BTreeMap<String, usize>,
    /// Lines of each file that are test code (`#[cfg(test)]` items).
    pub test_lines: BTreeMap<String, usize>,
    /// Per file: names brought in by `use`, with the path each stands for.
    pub uses: BTreeMap<String, BTreeMap<String, Vec<String>>>,
    /// Macros invoked where items go. Whatever functions they define are
    /// not seen.
    pub item_macros: Vec<Hit>,
    /// Modules declared `pub`.
    pub public_modules: BTreeSet<Vec<String>>,
    /// Every `pub use`.
    pub reexports: Vec<Reexport>,
    /// Structs, enums and traits, each with the module it is defined in.
    pub items: Vec<(Vec<String>, String)>,
}

/// A `pub use`: where it is written, the name it gives (`None` for a
/// glob), and the path it names, as written.
#[derive(Debug, Clone)]
pub struct Reexport {
    pub module: Vec<String>,
    pub name: Option<String>,
    pub target: Vec<String>,
}

fn line(span: Span) -> usize {
    span.start().line
}

fn is_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path().is_ident("test")
            || (a.path().is_ident("cfg")
                && a.meta
                    .require_list()
                    .is_ok_and(|l| l.tokens.to_string().contains("test")))
    })
}

fn last_ident(path: &syn::Path) -> String {
    path.segments
        .last()
        .map(|s| s.ident.to_string())
        .unwrap_or_default()
}

fn type_name(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(p) => Some(last_ident(&p.path)),
        syn::Type::Reference(r) => type_name(&r.elem),
        _ => None,
    }
}

/// The calls that give exclusive access to what a cell or a lock holds.
const EXCLUSIVE_BORROWS: [&str; 3] = ["borrow_mut", "lock", "write"];

/// `self.field`, possibly behind `&mut` or parentheses: the field's name.
/// Also a field reached through `.borrow_mut()`, `.lock()` or `.write()` on
/// a field of `self` (with `.unwrap()` or `.expect(..)` after it), which is
/// how a type built on `RefCell` or a lock holds its data: then the name
/// of the field reached, and the call it was reached through.
fn self_field(expr: &syn::Expr) -> Option<(String, Option<&'static str>)> {
    let is_self = |e: &syn::Expr| matches!(e, syn::Expr::Path(p) if p.path.is_ident("self"));
    match expr {
        syn::Expr::Reference(r) => self_field(&r.expr),
        syn::Expr::Paren(p) => self_field(&p.expr),
        syn::Expr::Field(f) => {
            let syn::Member::Named(name) = &f.member else {
                return None;
            };
            if is_self(&f.base) {
                return Some((name.to_string(), None));
            }
            let mut inner = &*f.base;
            if let syn::Expr::MethodCall(m) = inner
                && (m.method == "unwrap" || m.method == "expect")
            {
                inner = &m.receiver;
            }
            let syn::Expr::MethodCall(m) = inner else {
                return None;
            };
            let through = EXCLUSIVE_BORROWS
                .iter()
                .find(|b| m.method == **b && m.args.is_empty())?;
            matches!(&*m.receiver, syn::Expr::Field(holder) if is_self(&holder.base))
                .then(|| (name.to_string(), Some(*through)))
        }
        _ => None,
    }
}

/// How a take is described: `mem::take`, or `mem::take through .lock()`.
fn how_taken(how: &str, through: Option<&str>) -> String {
    match through {
        Some(call) => format!("{how} through .{call}()"),
        None => how.to_string(),
    }
}

/// Collects one function's facts from its body.
struct Body<'a> {
    file: &'a str,
    f: &'a mut Function,
    locals: BTreeSet<String>,
}

impl Body<'_> {
    fn implicit(&mut self, method: &str) {
        self.f.calls.push(Call::Method {
            name: method.to_string(),
            on_self: false,
        });
    }

    fn hit(&self, what: impl Into<String>, span: Span) -> Hit {
        Hit {
            what: what.into(),
            file: self.file.to_string(),
            line: line(span),
        }
    }
}

impl<'ast> Visit<'ast> for Body<'_> {
    fn visit_pat_ident(&mut self, p: &'ast syn::PatIdent) {
        self.locals.insert(p.ident.to_string());
        visit::visit_pat_ident(self, p);
    }

    fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
        let name = m.method.to_string();
        if COPYING_METHODS.contains(&name.as_str()) {
            self.f
                .copying
                .push(self.hit(format!(".{name}()"), m.method.span()));
        }
        if name == "take"
            && m.args.is_empty()
            && let Some((field, through)) = self_field(&m.receiver)
        {
            self.f.takes.push(Take {
                field,
                how: how_taken("Option::take", through),
                line: line(m.method.span()),
            });
        }
        if name == "spawn" || name == "spawn_scoped" {
            self.f
                .threads
                .push(self.hit(format!(".{name}(..)"), m.method.span()));
        }
        let on_self = matches!(&*m.receiver, syn::Expr::Path(p) if p.path.is_ident("self"));
        // A conversion or a formatting call runs an implementation under
        // another name.
        for (written, runs) in [
            ("into", "from"),
            ("try_into", "try_from"),
            ("to_string", "fmt"),
        ] {
            if name == written {
                self.implicit(runs);
            }
        }
        self.f.calls.push(Call::Method { name, on_self });
        visit::visit_expr_method_call(self, m);
    }

    fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
        match &*c.func {
            syn::Expr::Path(p) => {
                let path: Vec<String> = p
                    .path
                    .segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect();
                let name = path.last().cloned().unwrap_or_default();
                let before = path
                    .len()
                    .checked_sub(2)
                    .map(|i| path[i].as_str())
                    .unwrap_or("");
                let span = p
                    .path
                    .segments
                    .last()
                    .map_or(Span::call_site(), |s| s.ident.span());
                if (name == "take" || name == "replace" || name == "swap")
                    && (before == "mem" || path.len() == 1)
                {
                    if let Some((field, through)) = c.args.first().and_then(self_field) {
                        self.f.takes.push(Take {
                            field,
                            how: how_taken(&format!("mem::{name}"), through),
                            line: line(span),
                        });
                    }
                } else if ["Clone", "ToOwned", "ToString"].contains(&before)
                    && COPYING_METHODS.contains(&name.as_str())
                {
                    self.f
                        .copying
                        .push(self.hit(format!("{before}::{name}(..)"), span));
                } else if (before == "thread" && ["spawn", "scope"].contains(&name.as_str()))
                    || path
                        .iter()
                        .any(|s| s.contains("ThreadPool") || s == "Runtime")
                    || (["rayon", "tokio"].contains(&path[0].as_str())
                        && ["spawn", "scope", "join"].contains(&name.as_str()))
                {
                    self.f
                        .threads
                        .push(self.hit(format!("{}(..)", path.join("::")), span));
                }
                if path.len() == 1 && self.locals.contains(&name) {
                    // A parameter or local being called: a closure or a
                    // function value. Its body is somewhere else.
                    self.f.not_followed.push(self.hit(
                        format!(
                            "call through the value `{name}` (a closure or function passed in)"
                        ),
                        span,
                    ));
                } else {
                    self.f.calls.push(Call::Path(path, line(span)));
                }
            }
            other => {
                let span = syn::spanned::Spanned::span(other);
                self.f.not_followed.push(self.hit(
                    "call through an expression (a stored closure or function pointer)",
                    span,
                ));
            }
        }
        visit::visit_expr_call(self, c);
    }

    // Calls the syntax does not show as calls. Each is followed, by name,
    // to every implementation of that name in the crate.
    fn visit_expr_binary(&mut self, b: &'ast syn::ExprBinary) {
        use syn::BinOp::*;
        let method = match b.op {
            Add(_) => "add",
            Sub(_) => "sub",
            Mul(_) => "mul",
            Div(_) => "div",
            Rem(_) => "rem",
            BitAnd(_) => "bitand",
            BitOr(_) => "bitor",
            BitXor(_) => "bitxor",
            Shl(_) => "shl",
            Shr(_) => "shr",
            AddAssign(_) => "add_assign",
            SubAssign(_) => "sub_assign",
            MulAssign(_) => "mul_assign",
            DivAssign(_) => "div_assign",
            Eq(_) | Ne(_) => "eq",
            Lt(_) | Le(_) | Gt(_) | Ge(_) => "partial_cmp",
            _ => "",
        };
        if !method.is_empty() {
            self.implicit(method);
        }
        visit::visit_expr_binary(self, b);
    }

    fn visit_expr_unary(&mut self, u: &'ast syn::ExprUnary) {
        match u.op {
            syn::UnOp::Deref(_) => {
                self.implicit("deref");
                self.implicit("deref_mut");
            }
            syn::UnOp::Neg(_) => self.implicit("neg"),
            syn::UnOp::Not(_) => self.implicit("not"),
            _ => {}
        }
        visit::visit_expr_unary(self, u);
    }

    fn visit_expr_index(&mut self, i: &'ast syn::ExprIndex) {
        self.implicit("index");
        self.implicit("index_mut");
        visit::visit_expr_index(self, i);
    }

    fn visit_expr_for_loop(&mut self, l: &'ast syn::ExprForLoop) {
        self.implicit("into_iter");
        self.implicit("next");
        visit::visit_expr_for_loop(self, l);
    }

    fn visit_expr_assign(&mut self, a: &'ast syn::ExprAssign) {
        if let Some((field, _)) = self_field(&a.left) {
            self.f.assigned_self_fields.insert(field);
        }
        visit::visit_expr_assign(self, a);
    }

    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        let name = last_ident(&m.path);
        let span = m
            .path
            .segments
            .last()
            .map_or(Span::call_site(), |s| s.ident.span());
        if [
            "format",
            "println",
            "eprintln",
            "print",
            "eprint",
            "write",
            "writeln",
            "panic",
            "format_args",
        ]
        .contains(&name.as_str())
        {
            self.implicit("fmt");
        }
        if ALLOCATING_MACROS.contains(&name.as_str()) {
            self.f
                .allocating_macros
                .push(self.hit(format!("{name}!"), span));
        }
        if !KNOWN_MACROS.contains(&name.as_str()) {
            self.f
                .not_followed
                .push(self.hit(format!("macro `{name}!` is not expanded"), span));
        }
        // The arguments of the usual macros are expressions; read them as
        // such, so a `.clone()` inside `vec![..]` or `println!(..)` counts.
        let parser = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
        let semi = |input: syn::parse::ParseStream| -> syn::Result<Vec<syn::Expr>> {
            // `vec![value; count]`
            let first: syn::Expr = input.parse()?;
            input.parse::<syn::Token![;]>()?;
            Ok(vec![first, input.parse()?])
        };
        if let Ok(args) = m.parse_body_with(parser) {
            args.iter().for_each(|e| self.visit_expr(e));
        } else if let Ok(args) = m.parse_body_with(semi) {
            args.iter().for_each(|e| self.visit_expr(e));
        } else if KNOWN_MACROS.contains(&name.as_str()) && !m.tokens.is_empty() {
            self.f.not_followed.push(self.hit(
                format!("the arguments of `{name}!` could not be read"),
                span,
            ));
        }
    }
}

/// Walks a file: registers functions, shared-ownership types, `unsafe`,
/// and sizes.
struct File<'a> {
    file: String,
    module: Vec<String>,
    self_ty: Option<String>,
    impl_trait: Option<Vec<String>>,
    self_written: Vec<String>,
    self_args: Vec<&'static str>,
    item: String,
    out: &'a mut Crate,
}

impl File<'_> {
    fn function(&mut self, ident: &syn::Ident, sig: &syn::Signature, block: &syn::Block) {
        let short = ident.to_string();
        let mut parts = self.module.clone();
        parts.extend(self.self_ty.clone());
        parts.push(short.clone());
        let start = line(sig.fn_token.span);
        let mut f = Function {
            name: parts.join("::"),
            short,
            self_ty: self.self_ty.clone(),
            module: self.module.clone(),
            impl_trait: self.impl_trait.clone(),
            self_written: self.self_written.clone(),
            self_args: self.self_args.clone(),
            receiver: sig.receiver().map(|r| match &r.kind {
                syn::ReceiverKind::Reference(_, _, Some(_)) => "exclusive",
                syn::ReceiverKind::Reference(_, _, None) => "shared",
                _ => "value",
            }),
            returns_collection: matches!(&sig.output, syn::ReturnType::Type(_, ty)
                if matches!(&**ty, syn::Type::Path(p) if OWNED_COLLECTIONS.contains(&last_ident(&p.path).as_str()))),
            file: self.file.clone(),
            line: start,
            lines: block
                .brace_token
                .span
                .close()
                .end()
                .line
                .saturating_sub(start)
                + 1,
            ..Function::default()
        };
        let mut body = Body {
            file: &self.file,
            f: &mut f,
            locals: BTreeSet::new(),
        };
        sig.inputs.iter().for_each(|arg| body.visit_fn_arg(arg));
        body.visit_block(block);
        if matches!(sig.safety, syn::Safety::Unsafe(_)) {
            self.out.unsafe_uses.push(Hit {
                what: format!("unsafe fn {}", f.name),
                file: self.file.clone(),
                line: start,
            });
        }
        let previous = std::mem::replace(&mut self.item, format!("fn {}", f.name));
        self.out.functions.push(f);
        // Types and `unsafe` blocks inside the body.
        visit::visit_block(self, block);
        self.item = previous;
    }

    fn shared(&mut self, construct: &str, span: Span) {
        self.out.shared.push(Shared {
            construct: construct.to_string(),
            item: self.item.clone(),
            file: self.file.clone(),
            line: line(span),
        });
    }
}

/// `Outer<Inner<..>>` for the given names, in a type.
fn wraps(path: &syn::Path, outer: &str, inner: &[&str]) -> bool {
    let Some(last) = path.segments.last().filter(|s| s.ident == outer) else {
        return false;
    };
    let syn::PathArguments::AngleBracketed(args) = &last.arguments else {
        return false;
    };
    args.args.iter().any(|a| matches!(a, syn::GenericArgument::Type(t) if type_name(t).is_some_and(|n| inner.contains(&n.as_str()))))
}

/// Whether a type names something its holder can be changed through from
/// behind a shared reference.
struct MentionsCell(bool);

impl<'ast> Visit<'ast> for MentionsCell {
    fn visit_type_path(&mut self, t: &'ast syn::TypePath) {
        let name = last_ident(&t.path);
        self.0 |= ["Cell", "RefCell", "Mutex", "RwLock"].contains(&name.as_str())
            || name.starts_with("Atomic");
        visit::visit_type_path(self, t);
    }
}

impl<'ast> Visit<'ast> for File<'_> {
    fn visit_item_use(&mut self, u: &'ast syn::ItemUse) {
        fn flatten(
            tree: &syn::UseTree,
            prefix: &mut Vec<String>,
            out: &mut BTreeMap<String, Vec<String>>,
        ) {
            match tree {
                syn::UseTree::Path(p) => {
                    prefix.push(p.ident.to_string());
                    flatten(&p.tree, prefix, out);
                    prefix.pop();
                }
                syn::UseTree::Name(n) => {
                    let mut full = prefix.clone();
                    full.push(n.ident.to_string());
                    out.insert(n.ident.to_string(), full);
                }
                syn::UseTree::Rename(r) => {
                    let mut full = prefix.clone();
                    full.push(r.ident.to_string());
                    out.insert(r.rename.to_string(), full);
                }
                syn::UseTree::Group(g) => g.items.iter().for_each(|t| flatten(t, prefix, out)),
                syn::UseTree::Glob(_) => {
                    // Kept under a name no item can have.
                    out.insert(format!("*{}", prefix.join("::")), prefix.clone());
                }
            }
        }
        let mut found = BTreeMap::new();
        flatten(&u.tree, &mut Vec::new(), &mut found);
        if matches!(u.vis, syn::Visibility::Public(_)) {
            for (name, target) in &found {
                self.out.reexports.push(Reexport {
                    module: self.module.clone(),
                    name: (!name.starts_with('*')).then(|| name.clone()),
                    target: target.clone(),
                });
            }
        }
        let names = self.out.uses.entry(self.file.clone()).or_default();
        names.extend(found.into_iter().filter(|(name, _)| !name.starts_with('*')));
    }

    fn visit_item_macro(&mut self, m: &'ast syn::ItemMacro) {
        let name = last_ident(&m.mac.path);
        if name != "macro_rules" && name != "thread_local" {
            let span = m
                .mac
                .path
                .segments
                .last()
                .map_or(Span::call_site(), |s| s.ident.span());
            self.out.item_macros.push(Hit {
                what: format!("{name}!"),
                file: self.file.clone(),
                line: line(span),
            });
        }
    }

    fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
        if is_test(&m.attrs) {
            if let Some((brace, _)) = &m.content {
                let lines = brace
                    .span
                    .close()
                    .end()
                    .line
                    .saturating_sub(line(m.mod_token.span))
                    + 1;
                *self.out.test_lines.entry(self.file.clone()).or_default() += lines;
            }
            return;
        }
        self.module.push(m.ident.to_string());
        if matches!(m.vis, syn::Visibility::Public(_)) {
            self.out.public_modules.insert(self.module.clone());
        }
        visit::visit_item_mod(self, m);
        self.module.pop();
    }

    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        if !is_test(&f.attrs) {
            self.function(&f.sig.ident, &f.sig, &f.block);
        }
    }

    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        if is_test(&i.attrs) {
            return;
        }
        let name = type_name(&i.self_ty).unwrap_or_else(|| "?".to_string());
        *self.out.impl_blocks.entry(name.clone()).or_default() += 1;
        if i.unsafety.is_some() {
            self.out.unsafe_uses.push(Hit {
                what: format!("unsafe impl for {name}"),
                file: self.file.clone(),
                line: line(i.impl_token.span),
            });
        }
        let previous = self.self_ty.replace(name);
        let segments = |p: &syn::Path| {
            p.segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>()
        };
        self.impl_trait = i.trait_.as_ref().map(|(path, _)| segments(path));
        if let syn::Type::Path(t) = &*i.self_ty {
            self.self_written = segments(&t.path);
            if let Some(syn::PathArguments::AngleBracketed(a)) =
                t.path.segments.last().map(|s| &s.arguments)
            {
                self.self_args = a
                    .args
                    .iter()
                    .map(|g| {
                        if matches!(g, syn::GenericArgument::Lifetime(_)) {
                            "'_"
                        } else {
                            "_"
                        }
                    })
                    .collect();
            }
        }
        for item in &i.items {
            if let syn::ImplItem::Fn(f) = item
                && !is_test(&f.attrs)
            {
                self.function(&f.sig.ident, &f.sig, &f.block);
            }
        }
        self.self_ty = previous;
        self.impl_trait = None;
        self.self_written.clear();
        self.self_args.clear();
    }

    fn visit_item_enum(&mut self, e: &'ast syn::ItemEnum) {
        if !is_test(&e.attrs) {
            self.out
                .items
                .push((self.module.clone(), e.ident.to_string()));
        }
        visit::visit_item_enum(self, e);
    }

    fn visit_item_trait(&mut self, t: &'ast syn::ItemTrait) {
        self.out
            .items
            .push((self.module.clone(), t.ident.to_string()));
        let previous = self.self_ty.replace(t.ident.to_string());
        for item in &t.items {
            if let syn::TraitItem::Fn(f) = item
                && let Some(block) = &f.default
            {
                self.function(&f.sig.ident, &f.sig, block);
            }
        }
        self.self_ty = previous;
    }

    fn visit_item_struct(&mut self, s: &'ast syn::ItemStruct) {
        if is_test(&s.attrs) {
            return;
        }
        self.out
            .struct_fields
            .insert(s.ident.to_string(), s.fields.len());
        self.out
            .items
            .push((self.module.clone(), s.ident.to_string()));
        let mut cells = MentionsCell(false);
        cells.visit_fields(&s.fields);
        if cells.0 {
            self.out.cell_structs.insert(s.ident.to_string());
        }
        let previous = std::mem::replace(&mut self.item, format!("struct {}", s.ident));
        visit::visit_item_struct(self, s);
        self.item = previous;
    }

    fn visit_type_path(&mut self, t: &'ast syn::TypePath) {
        let span = t
            .path
            .segments
            .last()
            .map_or(Span::call_site(), |s| s.ident.span());
        if wraps(&t.path, "Rc", &["RefCell", "Cell"]) {
            self.shared("Rc<RefCell<..>>", span);
        } else if wraps(&t.path, "Arc", &["Mutex", "RwLock"]) {
            self.shared("Arc<Mutex<..>>", span);
        } else if last_ident(&t.path) == "Weak" {
            self.shared("Weak<..>", span);
        } else if let Some(last) = t.path.segments.last()
            && (last.ident == "Rc" || last.ident == "Arc")
            && let syn::PathArguments::AngleBracketed(args) = &last.arguments
            && let Some(syn::GenericArgument::Type(inner)) = args.args.first()
            && let Some(inner) = type_name(inner)
        {
            // Whether this is shared ownership depends on the fields of the
            // type inside, which may be declared in a file not yet read.
            let construct = last.ident.to_string();
            let item = self.item.clone();
            let file = self.file.clone();
            let line = line(span);
            self.out.shared_by_type.push((
                inner,
                Shared {
                    construct,
                    item,
                    file,
                    line,
                },
            ));
        }
        visit::visit_type_path(self, t);
    }

    fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
        // `Rc::new(RefCell::new(..))` with no type written anywhere.
        let ctor = |e: &syn::Expr, ty: &[&str]| {
            matches!(e, syn::Expr::Call(c) if matches!(&*c.func, syn::Expr::Path(p)
            if p.path.segments.len() == 2 && ty.contains(&p.path.segments[0].ident.to_string().as_str()) && p.path.segments[1].ident == "new"))
        };
        if ctor(&syn::Expr::Call(c.clone()), &["Rc"])
            && c.args
                .first()
                .is_some_and(|a| ctor(a, &["RefCell", "Cell"]))
        {
            self.shared("Rc<RefCell<..>>", syn::spanned::Spanned::span(&c.func));
        } else if ctor(&syn::Expr::Call(c.clone()), &["Arc"])
            && c.args
                .first()
                .is_some_and(|a| ctor(a, &["Mutex", "RwLock"]))
        {
            self.shared("Arc<Mutex<..>>", syn::spanned::Spanned::span(&c.func));
        }
        visit::visit_expr_call(self, c);
    }

    fn visit_expr_unsafe(&mut self, u: &'ast syn::ExprUnsafe) {
        self.out.unsafe_uses.push(Hit {
            what: format!("unsafe block in {}", self.item),
            file: self.file.clone(),
            line: line(u.unsafe_token.span),
        });
        visit::visit_expr_unsafe(self, u);
    }
}

impl Crate {
    /// Parses one file. `file` is its path as it should appear in the
    /// report; `module` is its module path (`a::b` for `src/a/b.rs`).
    /// Everything counted as shared ownership (check S7): what is shared
    /// by its spelling, then each `Rc<T>` or `Arc<T>` where `T` is a struct
    /// of the crate with cell or lock fields. Call it when every file has
    /// been added. Types are matched by name alone.
    pub fn shared_ownership(&self) -> Vec<Shared> {
        let by_type = self
            .shared_by_type
            .iter()
            .filter(|(inner, _)| self.cell_structs.contains(inner))
            .map(|(inner, s)| Shared {
                construct: format!("{}<{inner}> ({inner} has cell or lock fields)", s.construct),
                ..s.clone()
            });
        self.shared.iter().cloned().chain(by_type).collect()
    }

    pub fn add_file(&mut self, file: &str, module: &[String], source: &str) -> Result<(), String> {
        let parsed =
            syn::parse_file(source).map_err(|e| format!("{file}:{}: {e}", line(e.span())))?;
        self.file_lines
            .insert(file.to_string(), source.lines().count());
        if module.is_empty() {
            for attr in &parsed.attrs {
                let lint = ["forbid", "deny"]
                    .into_iter()
                    .find(|l| attr.path().is_ident(l));
                if let Some(lint) = lint
                    && attr
                        .meta
                        .require_list()
                        .is_ok_and(|l| l.tokens.to_string().contains("unsafe_code"))
                {
                    self.unsafe_lint = Some(lint.to_string());
                }
            }
        }
        let mut walker = File {
            file: file.to_string(),
            module: module.to_vec(),
            self_ty: None,
            impl_trait: None,
            self_written: Vec::new(),
            self_args: Vec::new(),
            item: "the top of the file".to_string(),
            out: self,
        };
        walker.visit_file(&parsed);
        Ok(())
    }

    /// Reads every `.rs` file under `src`. The module of a file is taken
    /// from its path; `lib.rs`, `main.rs` and `mod.rs` name their directory.
    ///
    /// Returns the crate, and separately whatever lives in files that are
    /// only compiled for tests (`#[cfg(test)] mod name;`): those are not
    /// part of the port, but a reader still wants their `unsafe` listed.
    pub fn read(src: &Path, shown_prefix: &str) -> Result<(Crate, Crate), String> {
        fn files(
            dir: &Path,
            module: &mut Vec<String>,
            shown: &str,
            out: &mut Vec<(String, Vec<String>, String)>,
        ) -> Result<(), String> {
            let mut entries: Vec<_> = std::fs::read_dir(dir)
                .map_err(|e| format!("{}: {e}", dir.display()))?
                .flatten()
                .collect();
            entries.sort_by_key(|e| e.file_name());
            for entry in entries {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                if path.is_dir() {
                    module.push(name.clone());
                    files(&path, module, &format!("{shown}/{name}"), out)?;
                    module.pop();
                } else if let Some(stem) = name.strip_suffix(".rs") {
                    let source = std::fs::read_to_string(&path)
                        .map_err(|e| format!("{}: {e}", path.display()))?;
                    let mut here = module.clone();
                    if !["lib", "main", "mod"].contains(&stem) {
                        here.push(stem.to_string());
                    }
                    out.push((format!("{shown}/{name}"), here, source));
                }
            }
            Ok(())
        }
        let mut all = Vec::new();
        files(src, &mut Vec::new(), shown_prefix, &mut all)?;
        // Modules declared `#[cfg(test)] mod name;` in their parent.
        let mut test_modules: Vec<Vec<String>> = Vec::new();
        for (file, module, source) in &all {
            let parsed =
                syn::parse_file(source).map_err(|e| format!("{file}:{}: {e}", line(e.span())))?;
            for item in &parsed.items {
                if let syn::Item::Mod(m) = item
                    && m.content.is_none()
                    && is_test(&m.attrs)
                {
                    let mut path = module.clone();
                    path.push(m.ident.to_string());
                    test_modules.push(path);
                }
            }
        }
        let (mut main, mut tests) = (Crate::default(), Crate::default());
        for (file, module, source) in &all {
            let test_only = test_modules.iter().any(|t| module.starts_with(t));
            (if test_only { &mut tests } else { &mut main }).add_file(file, module, source)?;
        }
        Ok((main, tests))
    }

    /// The functions whose qualified name is `entry` or ends with
    /// `::entry`.
    pub fn find(&self, entry: &str) -> Vec<usize> {
        // The whole path, when it is one, is that function and no other.
        let exact: Vec<usize> = (0..self.functions.len())
            .filter(|i| self.functions[*i].name == entry)
            .collect();
        if !exact.is_empty() {
            return exact;
        }
        let suffix = format!("::{entry}");
        (0..self.functions.len())
            .filter(|i| {
                self.functions[*i].name == entry || self.functions[*i].name.ends_with(&suffix)
            })
            .collect()
    }

    /// Functions a call may reach. By name, since nothing is type-checked:
    ///
    /// - `self.f()` in an `impl` of `T`: the `f` of `T` in the same file if
    ///   there is one;
    /// - any other `x.f()`: every method named `f` in the same file, or, if
    ///   the file has none, every method named `f` in the crate. This is how
    ///   a call through a trait or a generic is followed: to every
    ///   implementation of that name, which may be more than the real ones;
    /// - `T::f()`, `Self::f()`, `module::f()`: the `f` of that type or
    ///   module;
    /// - `f()`: the free function `f` in the same file, else in the crate.
    ///
    /// Functions outside the crate (the standard library, dependencies) are
    /// not looked into.
    fn callees(&self, from: &Function, call: &Call) -> Vec<usize> {
        let all = || 0..self.functions.len();
        let prefer_same_file = |found: Vec<usize>| {
            let near: Vec<usize> = found
                .iter()
                .copied()
                .filter(|i| self.functions[*i].file == from.file)
                .collect();
            if near.is_empty() { found } else { near }
        };
        match call {
            Call::Method { name, on_self } => {
                let methods: Vec<usize> = all()
                    .filter(|i| {
                        self.functions[*i].self_ty.is_some() && &self.functions[*i].short == name
                    })
                    .collect();
                if *on_self {
                    let own: Vec<usize> = methods
                        .iter()
                        .copied()
                        .filter(|i| {
                            self.functions[*i].self_ty == from.self_ty
                                && self.functions[*i].file == from.file
                        })
                        .collect();
                    if !own.is_empty() {
                        return own;
                    }
                }
                prefer_same_file(methods)
            }
            Call::Path(path, _) => {
                let name = path.last().map(String::as_str).unwrap_or("");
                let named = |i: &usize| self.functions[*i].short == name;
                if path.len() >= 2 {
                    let owner = &path[path.len() - 2];
                    let owner = if owner == "Self" {
                        from.self_ty.clone().unwrap_or_default()
                    } else {
                        owner.clone()
                    };
                    let of_type: Vec<usize> = all()
                        .filter(named)
                        .filter(|i| self.functions[*i].self_ty.as_deref() == Some(owner.as_str()))
                        .collect();
                    if !of_type.is_empty() {
                        return prefer_same_file(of_type);
                    }
                    let in_module = format!("{owner}::{name}");
                    return all()
                        .filter(|i| {
                            self.functions[*i].self_ty.is_none()
                                && (self.functions[*i].name == in_module
                                    || self.functions[*i].name.ends_with(&format!("::{in_module}")))
                        })
                        .collect();
                }
                prefer_same_file(
                    all()
                        .filter(named)
                        .filter(|i| self.functions[*i].self_ty.is_none())
                        .collect(),
                )
            }
        }
    }

    /// A path written in `module` as a path from the crate root, or `None`
    /// when it leads out of the crate.
    fn absolute(
        &self,
        module: &[String],
        written: &[String],
        dependencies: &[String],
    ) -> Option<Vec<String>> {
        let mut base = module.to_vec();
        let mut rest = written;
        match rest.first().map(String::as_str) {
            Some("crate") => {
                base.clear();
                rest = &rest[1..];
            }
            Some("std" | "core" | "alloc") => return None,
            Some(first) if dependencies.iter().any(|d| d == first) => return None,
            _ => {}
        }
        while let Some(first) = rest.first() {
            match first.as_str() {
                "self" => {}
                "super" => {
                    base.pop();
                }
                _ => break,
            }
            rest = &rest[1..];
        }
        base.extend(rest.iter().cloned());
        Some(base)
    }

    /// A path by which the item at `full` (from the crate root) can be
    /// named from outside the crate: its own, if every module on it is
    /// public, or one given by a `pub use`. `None` if the tool finds none.
    pub fn public_path(&self, full: &[String], dependencies: &[String]) -> Option<Vec<String>> {
        let modules_public =
            |path: &[String]| (1..path.len()).all(|n| self.public_modules.contains(&path[..n]));
        let mut seen = vec![full.to_vec()];
        let mut queue = vec![full.to_vec()];
        while let Some(path) = queue.pop() {
            if modules_public(&path) {
                return Some(path);
            }
            for r in &self.reexports {
                let Some(target) = self.absolute(&r.module, &r.target, dependencies) else {
                    continue;
                };
                if !path.starts_with(&target) || (r.name.is_none() && path.len() == target.len()) {
                    continue;
                }
                let mut via = r.module.clone();
                via.extend(r.name.clone());
                via.extend(path[target.len()..].iter().cloned());
                if !seen.contains(&via) && seen.len() < 64 {
                    seen.push(via.clone());
                    queue.push(via);
                }
            }
        }
        None
    }

    /// Where a type or trait named in `f`'s file is defined, from the
    /// crate root; `Err` with the path as written when it is outside the
    /// crate.
    fn defined_at(
        &self,
        f: &Function,
        written: &[String],
        dependencies: &[String],
    ) -> Result<Vec<String>, String> {
        let outside = || Err(written.join("::"));
        let Some(first) = written.first() else {
            return outside();
        };
        // A name brought in by `use` stands for the path it was given.
        let expanded: Vec<String> = match self.uses.get(&f.file).and_then(|u| u.get(first)) {
            Some(full) => full.iter().chain(&written[1..]).cloned().collect(),
            None => written.to_vec(),
        };
        let from_use = expanded != written;
        // Is `path` a module of this crate, as far as the tool has seen one?
        let is_module = |path: &[String]| {
            self.functions
                .iter()
                .map(|g| &g.module)
                .chain(self.items.iter().map(|(m, _)| m))
                .any(|m| m.starts_with(path))
        };
        let keyword = ["crate", "self", "super"].contains(&expanded[0].as_str());
        let start = if keyword || !from_use {
            self.absolute(&f.module, &expanded, dependencies)
        } else if is_module(&expanded[..1]) {
            // `use inner::Sim;` names a module at the crate root...
            self.absolute(&[], &expanded, dependencies)
        } else {
            // ...or one beside the file; anything else is another crate.
            let mut beside = f.module.clone();
            beside.push(expanded[0].clone());
            if is_module(&beside) {
                self.absolute(&f.module, &expanded, dependencies)
            } else {
                None
            }
        };
        let Some(path) = start else {
            return Err(expanded.join("::"));
        };
        let (module, name) = path.split_at(path.len() - 1);
        if self.items.iter().any(|(m, n)| m == module && n == &name[0]) {
            return Ok(path);
        }
        // Reached through a re-export or a glob: take the one definition
        // of that name, if there is exactly one.
        let named: Vec<&(Vec<String>, String)> =
            self.items.iter().filter(|(_, n)| n == &name[0]).collect();
        match named.as_slice() {
            [(m, n)] => Ok(m.iter().cloned().chain([n.clone()]).collect()),
            // Nothing of that name is defined in the crate: it comes from
            // the prelude or a glob import of another crate.
            [] => Err(written.join("::")),
            _ => Ok(path),
        }
    }

    /// The entry point as an expression the measuring program, which is
    /// another crate, can call: `krate::path::Type::method`,
    /// `<krate::path::Type as krate::path::Trait>::method`, or
    /// `krate::path::function`.
    pub fn callable(
        &self,
        f: &Function,
        krate: &str,
        dependencies: &[String],
    ) -> Result<String, String> {
        let reach = |full: Vec<String>, what: &str| {
            self.public_path(&full, dependencies).map(|p| format!("{krate}::{}", p.join("::"))).ok_or_else(|| {
                format!(
                    "{what} `{}` cannot be named from outside the crate: a module on its path is private and the tool found no `pub use` that exports it. The measuring program is another crate and can call only what is public. Add a `pub use`, or make the module public",
                    full.join("::")
                )
            })
        };
        let Some(ty) = &f.self_ty else {
            let mut full = f.module.clone();
            full.push(f.short.clone());
            return reach(full, "the entry point");
        };
        let written = if f.self_written.is_empty() {
            vec![ty.clone()]
        } else {
            f.self_written.clone()
        };
        let ty_path = match self.defined_at(f, &written, dependencies) {
            Ok(full) => reach(full, "the entry point's type")?,
            Err(outside) => {
                return Err(format!(
                    "the entry point `{}` is a method of `{outside}`, a type from outside the crate; declare a function of the crate as the hot path",
                    f.name
                ));
            }
        };
        match &f.impl_trait {
            None => Ok(format!("{ty_path}::{}", f.short)),
            Some(written) => {
                let trait_path = match self.defined_at(f, written, dependencies) {
                    Ok(full) => reach(full, "the trait the entry point belongs to")?,
                    Err(outside) => outside,
                };
                let args = if f.self_args.is_empty() {
                    String::new()
                } else {
                    format!("<{}>", f.self_args.join(", "))
                };
                Ok(format!("<{ty_path}{args} as {trait_path}>::{}", f.short))
            }
        }
    }

    /// Looks again at every call by path that reached no function of the
    /// crate, and either follows it or writes it down:
    ///
    /// - a name brought in by `use` that stands for a function of this
    ///   crate (`use crate::helpers::dup as d`) is followed to it;
    /// - a call into a dependency, written out or through `use`, is listed
    ///   as not followed, since the dependency's code is not read;
    /// - a call to a lower-case name that is no function of the crate, no
    ///   local and not imported is listed too: it was made by a macro or
    ///   came in by a glob import.
    ///
    /// `dependencies` are the crate names this crate depends on.
    pub fn resolve_calls(&mut self, dependencies: &[String]) {
        const HERE: [&str; 3] = ["crate", "self", "super"];
        for index in 0..self.functions.len() {
            let mut rewritten = Vec::new();
            let mut listed = Vec::new();
            let from = &self.functions[index];
            let uses = self.uses.get(&from.file);
            for (at, call) in from.calls.iter().enumerate() {
                let Call::Path(path, line) = call else {
                    continue;
                };
                if !self.callees(from, call).is_empty() {
                    continue;
                }
                let hit = |what: String| Hit {
                    what,
                    file: from.file.clone(),
                    line: *line,
                };
                let expanded: Option<Vec<String>> = uses
                    .and_then(|u| u.get(&path[0]))
                    .map(|full| full.iter().chain(&path[1..]).cloned().collect());
                let is_dependency = |name: &str| dependencies.iter().any(|d| d == name);
                match expanded {
                    Some(full)
                        if HERE.contains(&full[0].as_str())
                            || self.functions.iter().any(|f| {
                                Some(&f.short) == full.last() && !is_dependency(&full[0])
                            }) && !["std", "core", "alloc"].contains(&full[0].as_str()) =>
                    {
                        let inner: Vec<String> = full
                            .into_iter()
                            .filter(|s| !HERE.contains(&s.as_str()))
                            .collect();
                        rewritten.push((at, Call::Path(inner, *line)));
                    }
                    Some(full) if is_dependency(&full[0]) => {
                        listed.push(hit(format!(
                            "call into the dependency `{}` (written `{}`): its code is not read",
                            full.join("::"),
                            path.join("::")
                        )));
                    }
                    Some(_) => {}
                    None if path.len() >= 2 && is_dependency(&path[0]) => {
                        listed.push(hit(format!(
                            "call into the dependency `{}`: its code is not read",
                            path.join("::")
                        )));
                    }
                    None if path.len() == 1
                        && path[0].chars().next().is_some_and(char::is_lowercase)
                        && path[0] != "drop" =>
                    {
                        listed.push(hit(format!("call to `{}`, which is no function this tool can see: made by a macro, or brought in by a glob import", path[0])));
                    }
                    None => {}
                }
            }
            let f = &mut self.functions[index];
            for (at, call) in rewritten {
                f.calls[at] = call;
            }
            f.not_followed.extend(listed);
        }
    }

    /// Of the functions in `hot`: each that returns an owned collection and
    /// contains a copying construct or an allocating macro, with the number
    /// of calls to it written in the functions of `hot`. S3 counts such a
    /// function's copy once; it is made at every one of these calls, and
    /// how often each runs is not known from the source.
    pub fn call_sites_of_new_collections(&self, hot: &[usize]) -> Vec<(usize, usize)> {
        hot.iter()
            .copied()
            .filter(|i| {
                let f = &self.functions[*i];
                f.returns_collection && !(f.copying.is_empty() && f.allocating_macros.is_empty())
            })
            .map(|helper| {
                let sites = hot
                    .iter()
                    .map(|from| {
                        let from = &self.functions[*from];
                        from.calls
                            .iter()
                            .filter(|call| self.callees(from, call).contains(&helper))
                            .count()
                    })
                    .sum();
                (helper, sites)
            })
            .collect()
    }

    /// Every function reachable from `entry`, the entry included, in the
    /// order found. A `drop` written in the crate (an `impl Drop`) is
    /// counted as reachable from everywhere, since values are dropped
    /// without a call being written.
    pub fn reachable(&self, entry: usize) -> Vec<usize> {
        let mut seen = vec![entry];
        seen.extend((0..self.functions.len()).filter(|i| {
            *i != entry
                && self.functions[*i].short == "drop"
                && self.functions[*i].self_ty.is_some()
        }));
        let mut next = 0;
        while next < seen.len() {
            let from = &self.functions[seen[next]];
            for call in &from.calls {
                for callee in self.callees(from, call) {
                    if !seen.contains(&callee) {
                        seen.push(callee);
                    }
                }
            }
            next += 1;
        }
        seen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Crate {
        let mut c = Crate::default();
        c.add_file("src/lib.rs", &[], source).unwrap();
        c
    }

    fn reach(c: &Crate, entry: &str) -> Vec<String> {
        let found = c.find(entry);
        assert_eq!(found.len(), 1, "{entry}");
        c.reachable(found[0])
            .iter()
            .map(|i| c.functions[*i].name.clone())
            .collect()
    }

    fn copies(c: &Crate, name: &str) -> Vec<String> {
        c.functions[c.find(name)[0]]
            .copying
            .iter()
            .map(|h| h.what.clone())
            .collect()
    }

    #[test]
    fn a_clone_in_a_closure_written_inside_the_hot_function_is_counted_there() {
        let c = parse(
            "pub fn tick(v: &[String]) -> Vec<String> { v.iter().map(|s| s.clone()).collect() }",
        );
        assert_eq!(copies(&c, "tick"), [".collect()", ".clone()"]);
    }

    #[test]
    fn a_closure_passed_in_is_reported_as_not_followed() {
        // The clone is in the caller's closure. The hot function only calls
        // `f`, and where `f` comes from cannot be known from syntax.
        let c = parse(
            "pub fn tick(f: impl Fn(&String) -> String, s: &String) -> String { f(s) }
             pub fn caller(s: &String) -> String { tick(|x| x.clone(), s) }",
        );
        assert_eq!(reach(&c, "tick"), ["tick"]);
        assert!(copies(&c, "tick").is_empty());
        let tick = &c.functions[c.find("tick")[0]];
        assert_eq!(tick.not_followed.len(), 1);
        assert!(
            tick.not_followed[0]
                .what
                .contains("call through the value `f`")
        );
        // Looked at from the caller, the clone is found, since the closure
        // is written there.
        assert_eq!(copies(&c, "caller"), [".clone()"]);
    }

    #[test]
    fn a_stored_closure_is_reported_as_not_followed() {
        let c = parse(
            "pub struct S { cb: Box<dyn Fn()> } impl S { pub fn tick(&self) { (self.cb)() } }",
        );
        let tick = &c.functions[c.find("S::tick")[0]];
        assert!(
            tick.not_followed[0]
                .what
                .contains("call through an expression")
        );
    }

    #[test]
    fn a_call_through_a_trait_reaches_every_implementation_of_that_name() {
        let c = parse(
            "pub trait Shape { fn area(&self) -> String; }
             pub struct A; pub struct B;
             impl Shape for A { fn area(&self) -> String { String::new() } }
             impl Shape for B { fn area(&self) -> String { self.name().clone() } }
             impl B { fn name(&self) -> &String { todo!() } }
             pub fn tick(s: &dyn Shape) -> String { s.area() }
             pub fn generic<T: Shape>(s: &T) -> String { s.area() }",
        );
        // Which implementation runs is not known, so both are reached, and
        // the clone behind B's is found.
        for entry in ["tick", "generic"] {
            let reached = reach(&c, entry);
            assert!(
                reached.contains(&"A::area".to_string())
                    && reached.contains(&"B::area".to_string()),
                "{reached:?}"
            );
            assert!(reached.contains(&"B::name".to_string()));
        }
        assert_eq!(copies(&c, "B::area"), [".clone()"]);
    }

    #[test]
    fn calls_inside_the_usual_macros_are_read_and_other_macros_are_reported() {
        let c = parse(
            "macro_rules! mine { ($e:expr) => { $e.clone() } }
             pub fn tick(s: &String, n: usize) -> Vec<String> { println!(\"{}\", s.to_string()); let _ = mine!(s); vec![s.clone(); n] }",
        );
        assert_eq!(copies(&c, "tick"), [".to_string()", ".clone()"]);
        let tick = &c.functions[c.find("tick")[0]];
        assert_eq!(
            tick.allocating_macros
                .iter()
                .map(|h| h.what.as_str())
                .collect::<Vec<_>>(),
            ["vec!"]
        );
        assert!(
            tick.not_followed
                .iter()
                .any(|h| h.what == "macro `mine!` is not expanded")
        );
    }

    #[test]
    fn a_field_taken_out_of_self_and_put_back_is_seen() {
        let c = parse(
            "pub struct S { buf: Vec<u8>, slot: Option<u8>, other: Vec<u8> }
             impl S {
                 pub fn tick(&mut self) {
                     let mut b = std::mem::take(&mut self.buf); b.push(1); self.buf = b;
                     let s = self.slot.take(); self.slot = s;
                     let gone = std::mem::take(&mut self.other); drop(gone);
                 }
             }",
        );
        let tick = &c.functions[c.find("S::tick")[0]];
        let taken: Vec<(&str, &str)> = tick
            .takes
            .iter()
            .map(|t| (t.field.as_str(), t.how.as_str()))
            .collect();
        assert_eq!(
            taken,
            [
                ("buf", "mem::take"),
                ("slot", "Option::take"),
                ("other", "mem::take")
            ]
        );
        assert_eq!(
            tick.assigned_self_fields
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["buf", "slot"]
        );
    }

    #[test]
    fn a_field_taken_through_a_borrow_or_a_lock_on_a_field_of_self_is_seen() {
        // RUST-092: in a port built on `Rc<RefCell<..>>` this is the only
        // form taking a field out and putting it back can have.
        let c = parse(
            "pub struct Inner { listeners: Vec<u8>, slot: Option<u8>, queue: Vec<u8> }
             pub struct S(std::rc::Rc<std::cell::RefCell<Inner>>);
             pub struct L { inner: std::sync::Mutex<Inner>, state: std::sync::RwLock<Inner> }
             impl S {
                 pub fn tick(&self) {
                     let l = std::mem::take(&mut self.0.borrow_mut().listeners);
                     self.0.borrow_mut().listeners = l;
                     let s = self.0.borrow_mut().slot.take();
                     self.0.borrow_mut().slot = s;
                 }
             }
             impl L {
                 pub fn tick(&self, other: &Inner) {
                     let q = std::mem::take(&mut self.inner.lock().unwrap().queue);
                     self.inner.lock().unwrap().queue = q;
                     let l = std::mem::replace(&mut self.state.write().expect(\"lock\").listeners, Vec::new());
                     self.state.write().expect(\"lock\").listeners = l;
                     let _ = other.queue.len();
                 }
             }
             pub fn free(x: &std::cell::RefCell<Inner>) {
                 let q = std::mem::take(&mut x.borrow_mut().queue); x.borrow_mut().queue = q;
             }",
        );
        let seen = |name: &str| {
            let f = &c.functions[c.find(name)[0]];
            let taken: Vec<(String, String)> = f
                .takes
                .iter()
                .map(|t| (t.field.clone(), t.how.clone()))
                .collect();
            (taken, f.assigned_self_fields.clone())
        };
        let pair = |a: &str, b: &str| (a.to_string(), b.to_string());
        let set = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<BTreeSet<_>>();
        assert_eq!(
            seen("S::tick"),
            (
                vec![
                    pair("listeners", "mem::take through .borrow_mut()"),
                    pair("slot", "Option::take through .borrow_mut()")
                ],
                set(&["listeners", "slot"])
            )
        );
        assert_eq!(
            seen("L::tick"),
            (
                vec![
                    pair("queue", "mem::take through .lock()"),
                    pair("listeners", "mem::replace through .write()")
                ],
                set(&["listeners", "queue"])
            )
        );
        // Not a field of `self`: as before, nothing.
        assert_eq!(seen("free"), (vec![], set(&[])));
    }

    #[test]
    fn shared_ownership_of_a_type_with_cell_fields_is_found() {
        // RUST-093: the sharing of `Rc<RefCell<State>>` written as
        // `Rc<State>` with the cells inside `State`.
        let mut c = Crate::default();
        c.add_file(
            "src/lib.rs",
            &[],
            "use std::{rc::Rc, sync::Arc};
             mod state;
             pub struct Depot { zones: Vec<Rc<state::ZoneState>>, log: Arc<state::Log>, names: Rc<Names>, n: Rc<state::Counter> }
             pub struct Names { all: Vec<String> }
             pub fn first(d: &Depot) { let _z: Rc<state::ZoneState> = d.zones[0].clone(); }",
        )
        .unwrap();
        // The types are declared in a file read after the one that uses them.
        c.add_file(
            "src/state.rs",
            &["state".to_string()],
            "use std::{cell::{Cell, RefCell}, sync::{Mutex, atomic::AtomicU64}};
             pub struct ZoneState { load: Cell<u32>, waiting: RefCell<Vec<u32>> }
             pub struct Log { lines: Mutex<Vec<String>> }
             pub struct Counter(AtomicU64);",
        )
        .unwrap();
        let found: Vec<(String, String, usize)> = c
            .shared_ownership()
            .into_iter()
            .map(|s| (s.construct, s.item, s.line))
            .collect();
        let hit = |c: &str, item: &str, line| (c.to_string(), item.to_string(), line);
        assert_eq!(
            found,
            [
                hit(
                    "Rc<ZoneState> (ZoneState has cell or lock fields)",
                    "struct Depot",
                    3
                ),
                hit("Arc<Log> (Log has cell or lock fields)", "struct Depot", 3),
                hit(
                    "Rc<Counter> (Counter has cell or lock fields)",
                    "struct Depot",
                    3
                ),
                hit(
                    "Rc<ZoneState> (ZoneState has cell or lock fields)",
                    "fn first",
                    5
                ),
            ],
            "`Rc<Names>` shares data nobody can change through it: not counted"
        );
    }

    #[test]
    fn call_sites_of_a_helper_that_returns_a_new_collection_are_counted() {
        // RUST-094: the copy is written once, in the helper, and made at
        // every call.
        let c = parse(
            "pub struct S { bag: Vec<u32>, names: Vec<String> }
             impl S {
                 fn bag(&self) -> Vec<u32> { self.bag.to_vec() }
                 fn label(&self) -> String { format!(\"{}\", self.bag.len()) }
                 fn total(&self) -> u32 { self.bag.iter().cloned().sum() }
                 fn empty(&self) -> Vec<u32> { Vec::new() }
                 pub fn tick(&mut self) {
                     for b in self.bag() { let _ = b; }
                     let _ = (self.bag().len(), self.label(), self.total(), self.empty());
                     self.helper();
                 }
                 fn helper(&self) { let _ = self.bag(); }
                 pub fn elsewhere(&self) { let _ = self.bag(); }
             }",
        );
        let hot = c.reachable(c.find("S::tick")[0]);
        let counted: Vec<(&str, usize)> = c
            .call_sites_of_new_collections(&hot)
            .into_iter()
            .map(|(f, n)| (c.functions[f].name.as_str(), n))
            .collect();
        // `total` copies and returns a number; `empty` returns a collection
        // and copies nothing; `elsewhere` is not on the hot path.
        assert_eq!(counted, [("S::bag", 3), ("S::label", 1)]);
    }

    #[test]
    fn self_calls_stay_in_their_own_type_and_file() {
        let mut c = Crate::default();
        c.add_file(
            "src/a.rs",
            &["a".to_string()],
            "pub struct F; impl F { pub fn tick(&mut self) { self.step() } fn step(&mut self) {} }",
        )
        .unwrap();
        c.add_file("src/b.rs", &["b".to_string()], "pub struct F; impl F { pub fn tick(&mut self) { self.step() } fn step(&mut self) { let _ = String::new().clone(); } }").unwrap();
        assert_eq!(c.find("F::tick").len(), 2);
        assert_eq!(reach(&c, "a::F::tick"), ["a::F::tick", "a::F::step"]);
        assert_eq!(reach(&c, "b::F::tick"), ["b::F::tick", "b::F::step"]);
    }

    #[test]
    fn shared_ownership_unsafe_threads_and_tests_are_found() {
        let c = parse(
            "#![forbid(unsafe_code)]
             use std::{rc::{Rc, Weak}, cell::RefCell, sync::{Arc, Mutex}};
             pub struct Node { parent: Weak<RefCell<Node>>, kids: Vec<Rc<RefCell<Node>>>, lock: Arc<Mutex<u8>> }
             pub fn make() { let _ = Rc::new(RefCell::new(1)); }
             pub fn tick() { std::thread::scope(|s| { s.spawn(|| {}); }); let _ = unsafe { 1 }; }
             #[cfg(test)] mod tests { fn t() { let _ = String::new().clone(); } }",
        );
        let kinds: Vec<&str> = c.shared.iter().map(|s| s.construct.as_str()).collect();
        assert_eq!(
            kinds,
            [
                "Weak<..>",
                "Rc<RefCell<..>>",
                "Arc<Mutex<..>>",
                "Rc<RefCell<..>>"
            ]
        );
        assert_eq!(c.shared[0].item, "struct Node");
        assert_eq!(c.unsafe_lint.as_deref(), Some("forbid"));
        assert_eq!(c.unsafe_uses.len(), 1);
        let tick = &c.functions[c.find("tick")[0]];
        assert_eq!(tick.threads.len(), 2);
        assert!(c.find("t").is_empty(), "test code is not part of the port");
        assert_eq!(c.struct_fields["Node"], 3);
    }
}
