//! From clang's nodes to the catalog's functions (`ir`).

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::ast::{Kind, Literal, Node};
use crate::clang::Platform;
use crate::ir::{
    BinOp, Block, BlockId, Callee, Expr, ExprKind, Field, FnName, Function, Global, GlobalName,
    Init, InitItem, Place, Stmt, Term, Ty, UnOp, Var, VarId,
};
use crate::layout::Layouts;

/// The configured source tree in the image; its files are named from its
/// top (`Zend/zend.c`), other files by their path in the image.
const SOURCE_TREE: &str = "/usr/src/php/";

/// What a translation unit declares, by declaration id, and the names the
/// catalog gives its files, types and records.
pub(crate) struct TuIndex {
    /// The directory the image's files are under, as the dump's paths
    /// start.
    root: Arc<str>,
    paths: FxHashMap<Arc<str>, Arc<str>>,
    types: FxHashMap<Box<str>, Ty>,
    /// Each function's name as the program links it.
    pub(crate) functions: FxHashMap<u64, FnName>,
    /// The functions declared not to return.
    noreturn: FxHashSet<u64>,
    /// Each global variable's name as the program links it.
    pub(crate) globals: FxHashMap<u64, GlobalName>,
    enum_values: FxHashMap<u64, i128>,
    /// Each typedef's type.
    pub(crate) typedefs: FxHashMap<Box<str>, Ty>,
    /// Each record's members in order.
    records: FxHashMap<Arc<str>, Vec<Member>>,
    /// The catalog's name for each record the unit defines, by its
    /// declaration (`name_records`).
    decl_records: FxHashMap<u64, Arc<str>>,
    /// How many records of each name the unit defines.
    record_counts: FxHashMap<Arc<str>, u32>,
    /// Each member's record and name, by its declaration.
    members: FxHashMap<u64, (Arc<str>, Arc<str>)>,
    /// Each record's layout, as clang computed it for the unit.
    pub(crate) layouts: Layouts,
    /// Each enumeration's size, by its tag.
    enum_sizes: FxHashMap<Arc<str>, u64>,
    platform: Platform,
}

impl TuIndex {
    pub(crate) fn new(root: Arc<str>, layouts: Layouts, platform: Platform) -> TuIndex {
        TuIndex {
            root,
            layouts,
            enum_sizes: FxHashMap::default(),
            platform,
            paths: FxHashMap::default(),
            types: FxHashMap::default(),
            functions: FxHashMap::default(),
            noreturn: FxHashSet::default(),
            globals: FxHashMap::default(),
            enum_values: FxHashMap::default(),
            typedefs: FxHashMap::default(),
            records: FxHashMap::default(),
            decl_records: FxHashMap::default(),
            record_counts: FxHashMap::default(),
            members: FxHashMap::default(),
        }
    }

    /// A file as the catalog names it.
    pub(crate) fn path(&mut self, file: &Arc<str>) -> Arc<str> {
        if let Some(path) = self.paths.get(file) {
            return path.clone();
        }
        let path = catalog_path(&self.root, file);
        self.paths.insert(file.clone(), path.clone());
        path
    }

    pub(crate) fn node_path(&mut self, node: &Node) -> Arc<str> {
        match &node.loc.file {
            Some(file) => self.path(file),
            None => Arc::from("?"),
        }
    }

    /// A type's text as the catalog writes it, the paths of the unnamed
    /// records in it named as the catalog names files.
    pub(crate) fn ty(&mut self, text: &str) -> Ty {
        if let Some(ty) = self.types.get(text) {
            return ty.clone();
        }
        let ty: Ty = Arc::from(clean_type(&self.root, text));
        self.types.insert(text.into(), ty.clone());
        ty
    }

    fn node_ty(&mut self, node: &Node) -> Ty {
        match node.canonical_type() {
            Some(text) => {
                let text = text.to_owned();
                self.ty(&text)
            }
            None => self.ty("void"),
        }
    }

    /// The record a type names, through its qualifiers, pointers, arrays
    /// and typedefs: its tag, or for an unnamed record, `anon@` and where
    /// it is declared.
    pub(crate) fn record_of(&mut self, text: &str) -> Arc<str> {
        let mut text: String = self.ty(text).to_string();
        for _ in 0..32 {
            let stripped = strip_declarator(&text);
            if let Some(name) = anonymous_record(stripped) {
                return name;
            }
            for keyword in ["struct ", "union ", "enum "] {
                if let Some(tag) = stripped.strip_prefix(keyword) {
                    return Arc::from(tag.trim());
                }
            }
            match self.typedefs.get(stripped) {
                Some(under) if **under != *stripped => text = under.to_string(),
                _ => return Arc::from(stripped),
            }
        }
        Arc::from(strip_declarator(&text))
    }

    /// The name of a record a declaration declares.
    fn record_decl_name(&mut self, node: &Node) -> Arc<str> {
        match node.name.as_deref() {
            Some(name) => Arc::from(name),
            None => {
                let path = self.node_path(node);
                Arc::from(format!("anon@{path}:{}:{}", node.loc.line, node.loc.col))
            }
        }
    }

    /// Indexes the top-level declarations of a unit: each function and
    /// global by the name the program links it by, each typedef, record and
    /// enumerator.
    pub(crate) fn index(&mut self, nodes: &[Node]) {
        for node in nodes {
            self.name_records(node);
        }
        let mut static_functions: FxHashMap<&str, Arc<str>> = FxHashMap::default();
        let mut static_globals: FxHashMap<&str, Arc<str>> = FxHashMap::default();
        for node in nodes {
            let Some(name) = node.name.as_deref() else {
                continue;
            };
            match node.kind {
                Kind::FunctionDecl if is_static(node) && has_body(node) => {
                    let path = self.node_path(node);
                    static_functions.insert(name, path);
                }
                Kind::VarDecl if is_static(node) => {
                    let path = self.node_path(node);
                    if node.has_init {
                        static_globals.insert(name, path);
                    } else {
                        static_globals.entry(name).or_insert(path);
                    }
                }
                _ => {}
            }
        }
        for node in nodes {
            match node.kind {
                Kind::FunctionDecl => {
                    let Some(name) = node.name.as_deref() else {
                        continue;
                    };
                    let linked = if is_static(node) {
                        let path = match static_functions.get(name) {
                            Some(path) => path.clone(),
                            None => self.node_path(node),
                        };
                        Arc::from(format!("{path}:{name}"))
                    } else {
                        Arc::from(name)
                    };
                    if is_noreturn(node) {
                        self.noreturn.insert(node.id);
                    }
                    self.functions.insert(node.id, linked);
                }
                Kind::VarDecl => {
                    let Some(name) = node.name.as_deref() else {
                        continue;
                    };
                    let linked = if is_static(node) {
                        let path = match static_globals.get(name) {
                            Some(path) => path.clone(),
                            None => self.node_path(node),
                        };
                        Arc::from(format!("{path}:{name}"))
                    } else {
                        Arc::from(name)
                    };
                    self.globals.insert(node.id, linked);
                }
                _ => self.index_type(node),
            }
        }
        for pair in nodes.windows(2) {
            let [record, typedef] = pair else { continue };
            if record.kind == Kind::RecordDecl
                && record.name.is_none()
                && record.is_definition
                && typedef.kind == Kind::TypedefDecl
                && let Some(name) = typedef.name.as_deref()
                && let Some(ty) = typedef.ty.as_deref()
                && [format!("struct {name}"), format!("union {name}")]
                    .iter()
                    .any(|t| t == ty)
            {
                let anonymous = self.defined_record(record);
                self.alias_record(&anonymous, name);
            }
            if record.kind == Kind::EnumDecl
                && record.name.is_none()
                && typedef.kind == Kind::TypedefDecl
                && let Some(name) = typedef.name.as_deref()
                && typedef.ty.as_deref() == Some(format!("enum {name}").as_str())
            {
                let anonymous = self.record_decl_name(record);
                if let Some(size) = self.enum_sizes.get(&anonymous).copied() {
                    self.enum_sizes.insert(Arc::from(name), size);
                }
            }
        }
    }

    /// Names an unnamed record by the typedef declared with it, as clang
    /// names its type everywhere but in the layouts (`typedef struct {…}
    /// X;` is `struct X`).
    fn alias_record(&mut self, anonymous: &Arc<str>, name: &str) {
        let name: Arc<str> = Arc::from(name);
        if let Some(layout) = self.layouts.get(anonymous).cloned() {
            self.layouts.entry(name.clone()).or_insert(layout);
        }
        if let Some(members) = self.records.get(anonymous).cloned() {
            self.records.entry(name).or_insert(members);
        }
    }

    /// Names each record the unit defines, in the order clang completes
    /// them, which is the order it lays them out in: a record after the
    /// records its members declare. A record whose name an earlier one
    /// has is told by its number (`anon@ext/opcache/jit/ir/ir.h:488:2#2`):
    /// clang names an unnamed record by where the outermost macro
    /// expansion holding it is, which the records one expansion declares
    /// share, and records local to functions may share a tag.
    fn name_records(&mut self, node: &Node) {
        for child in &node.inner {
            self.name_records(child);
        }
        if let Some(filler) = &node.array_filler {
            self.name_records(filler);
        }
        if node.kind == Kind::RecordDecl && node.is_definition {
            let name = self.record_decl_name(node);
            let count = self.record_counts.entry(name.clone()).or_insert(0);
            *count += 1;
            let name = numbered(name, *count);
            self.decl_records.insert(node.id, name);
        }
    }

    /// The catalog's name for a record a declaration defines.
    fn defined_record(&mut self, node: &Node) -> Arc<str> {
        match self.decl_records.get(&node.id) {
            Some(name) => name.clone(),
            None => self.record_decl_name(node),
        }
    }

    /// A member's record and name, by its declaration.
    pub(crate) fn member(&self, decl: u64) -> Option<(Arc<str>, Arc<str>)> {
        self.members.get(&decl).cloned()
    }

    /// A record's members in order.
    pub(crate) fn record_members(&self, record: &str) -> Vec<Member> {
        self.records.get(record).cloned().unwrap_or_default()
    }

    /// Indexes a typedef, record or enumeration, at any scope.
    pub(crate) fn index_type(&mut self, node: &Node) {
        match node.kind {
            Kind::TypedefDecl => {
                if let (Some(name), Some(ty)) = (node.name.as_deref(), node.canonical_type()) {
                    let ty = ty.to_owned();
                    let ty = self.ty(&ty);
                    self.typedefs.insert(name.into(), ty);
                }
            }
            Kind::RecordDecl => self.index_record(node),
            Kind::EnumDecl => self.index_enum(node),
            _ => {}
        }
    }

    fn index_record(&mut self, node: &Node) {
        let mut members = Vec::new();
        let mut decls = Vec::new();
        // The records defined among the members, by the name their type
        // gives them.
        let mut defined: FxHashMap<Arc<str>, Arc<str>> = FxHashMap::default();
        for child in &node.inner {
            match child.kind {
                Kind::FieldDecl => {
                    let ty = child.canonical_type().unwrap_or("").to_owned();
                    let held = self.record_of(&ty);
                    let record = defined.get(&held).cloned();
                    let (name, anonymous) = match (child.name.as_deref(), &record) {
                        (Some(name), _) => (Arc::from(name), false),
                        (None, Some(record)) if child.is_implicit => (record.clone(), true),
                        (None, None) if child.is_implicit => (held, true),
                        (None, _) => continue,
                    };
                    decls.push((child.id, name.clone()));
                    members.push(Member {
                        name,
                        record,
                        anonymous,
                    });
                }
                Kind::RecordDecl => {
                    self.index_record(child);
                    if child.is_definition {
                        let held = self.record_decl_name(child);
                        let name = self.defined_record(child);
                        defined.insert(held, name);
                    }
                }
                Kind::EnumDecl => self.index_type(child),
                _ => {}
            }
        }
        if node.is_definition {
            let name = self.defined_record(node);
            for (decl, member) in decls {
                self.members.insert(decl, (name.clone(), member));
            }
            self.name_anonymous_fields(&name, &members);
            self.records.insert(name, members);
        }
    }

    /// Names a layout's anonymous members by their records as the unit
    /// names them, where the layout names them by the place clang names
    /// them by alone.
    fn name_anonymous_fields(&mut self, record: &str, members: &[Member]) {
        let Some(layout) = self.layouts.get_mut(record) else {
            return;
        };
        let mut anonymous = members.iter().filter(|m| m.anonymous);
        for field in &mut layout.fields {
            if !field.name.starts_with("anon@") {
                continue;
            }
            let Some(member) = anonymous.next() else {
                return;
            };
            if *unnumbered(&member.name) == *field.name {
                field.name = member.name.clone();
            }
        }
    }

    fn index_enum(&mut self, node: &Node) {
        let mut next: i128 = 0;
        let mut wide = false;
        for constant in &node.inner {
            if constant.kind != Kind::EnumConstantDecl {
                continue;
            }
            let value = constant
                .inner
                .iter()
                .find(|c| is_expression(&c.kind))
                .and_then(|c| self.const_eval(c))
                .unwrap_or(next);
            self.enum_values.insert(constant.id, value);
            wide |= value < i128::from(i32::MIN) || value > i128::from(u32::MAX);
            next = value + 1;
        }
        if node.is_definition || !node.inner.is_empty() {
            let name = self.record_decl_name(node);
            self.enum_sizes.insert(name, if wide { 8 } else { 4 });
        }
    }

    /// A type's size in bytes, as the unit's target lays it out.
    pub(crate) fn size_of(&mut self, text: &str) -> Option<u64> {
        self.size_align(text, 0).map(|(size, _)| size)
    }

    pub(crate) fn align_of(&mut self, text: &str) -> Option<u64> {
        self.size_align(text, 0).map(|(_, align)| align)
    }

    fn size_align(&mut self, text: &str, depth: u32) -> Option<(u64, u64)> {
        if depth > 32 {
            return None;
        }
        let text = strip_qualifiers(text);
        if text.ends_with('*') {
            return Some((8, 8));
        }
        if let Some(open) = text.find("(*") {
            // A pointer to a function or an array, or an array of them.
            let close = matching_paren(text, open)?;
            let inner = &text[open + 2..close];
            let mut count = 1;
            for bound in array_bounds(inner)? {
                count *= bound;
            }
            return Some((8 * count, 8));
        }
        if text.ends_with(']') {
            let open = text.rfind('[')?;
            let bound: u64 = text[open + 1..text.len() - 1].trim().parse().ok()?;
            let (size, align) = self.size_align(&text[..open], depth + 1)?;
            return Some((size * bound, align));
        }
        if text.ends_with(')') {
            // A function type: GNU C gives it a size of one.
            return Some((1, 1));
        }
        if let Some(scalar) = scalar_size(text, self.platform) {
            return Some(scalar);
        }
        if let Some(tag) = text.strip_prefix("enum ") {
            let size = self.enum_sizes.get(tag.trim()).copied().unwrap_or(4);
            return Some((size, size));
        }
        // An unnamed enumeration, by the typedef clang names it by.
        if let Some(size) = self.enum_sizes.get(text).copied() {
            return Some((size, size));
        }
        if let Some(vector) = text.strip_prefix("__attribute__((__vector_size__(") {
            let (count, rest) = vector.split_once(" * sizeof(")?;
            let (element, _) = rest.split_once(")))) ")?;
            let count: u64 = count.trim().parse().ok()?;
            let (size, _) = self.size_align(element, depth + 1)?;
            return Some((count * size, count * size));
        }
        let record = self.record_of(text);
        if let Some(layout) = self.layouts.get(&record) {
            return Some((layout.size, layout.align));
        }
        match self.typedefs.get(text).cloned() {
            Some(under) if *under != *text => self.size_align(&under, depth + 1),
            _ => None,
        }
    }

    /// The record and the member a member access names: by the member's
    /// declaration, else by the types.
    pub(crate) fn member_access(&mut self, node: &Node, base: &Node) -> (Arc<str>, Arc<str>) {
        if let Some(member) = node.member_decl.and_then(|decl| self.member(decl)) {
            return member;
        }
        let base_ty = base.canonical_type().unwrap_or("").to_owned();
        let record = self.record_of(&base_ty);
        let name = match node.name.as_deref() {
            Some(name) if !name.is_empty() => Arc::from(name),
            _ => {
                let own = node.canonical_type().unwrap_or("").to_owned();
                self.record_of(&own)
            }
        };
        (record, name)
    }

    /// A member as the catalog writes it, with its offset.
    pub(crate) fn field(&mut self, record: Arc<str>, name: Arc<str>) -> Field {
        let offset = self.field_offset(&record, &name).map(|(offset, _)| offset);
        Field {
            record,
            name,
            offset,
        }
    }

    /// The size of what a pointer of a type points to, or of an array's
    /// element; 1 for `void`, as GNU C counts it.
    pub(crate) fn pointee_size(&mut self, pointer: &str) -> Option<u64> {
        let resolved = self.ty(pointer).to_string();
        let mut text = strip_qualifiers(&resolved).to_owned();
        if let Some(under) = self.typedefs.get(text.as_str()).cloned() {
            text = strip_qualifiers(&under).to_owned();
        }
        let pointee = if let Some(inner) = text.strip_suffix('*') {
            inner.trim_end().to_owned()
        } else if let Some(at) = text.find("(*)") {
            format!("{}{}", text[..at].trim_end(), &text[at + 3..])
        } else if text.ends_with(']') {
            array_element(&text)
        } else {
            return None;
        };
        if strip_qualifiers(&pointee) == "void" {
            return Some(1);
        }
        self.size_of(&pointee)
    }

    /// The offset of a member, through the members of anonymous records
    /// that hold it.
    fn field_offset(&mut self, record: &str, name: &str) -> Option<(u64, Arc<str>)> {
        let fields = self.layouts.get(record)?.fields.clone();
        for field in &fields {
            if &*field.name == name {
                return Some((field.offset, field.ty.clone()));
            }
        }
        for field in &fields {
            if field.name.starts_with("anon@")
                && *field.name != *record
                && let Some((offset, ty)) = self.field_offset(&field.name, name)
            {
                return Some((field.offset + offset, ty));
            }
        }
        None
    }

    /// The offset `offsetof` gives, from the member access the catalog's
    /// definition of it writes beside it: `&((t *)0)->d`.
    fn member_offset(&mut self, node: &Node) -> Option<u64> {
        match &node.kind {
            Kind::ParenExpr | Kind::ImplicitCastExpr => self.member_offset(node.inner.first()?),
            Kind::UnaryOperator if node.opcode.as_deref() == Some("&") => {
                self.member_offset(node.inner.first()?)
            }
            Kind::CStyleCastExpr => {
                let operand = node.inner.iter().find(|c| is_expression(&c.kind))?;
                (self.const_eval(operand)? == 0).then_some(0)
            }
            Kind::MemberExpr => {
                let base = node.inner.first()?;
                let base_offset = self.member_offset(base)?;
                let (record, name) = self.member_access(node, base);
                let (offset, _) = self.field_offset(&record, &name)?;
                Some(base_offset + offset)
            }
            Kind::ArraySubscriptExpr => {
                let [base, index] = node.inner.as_slice() else {
                    return None;
                };
                let base_offset = self.member_offset(base)?;
                let index = u64::try_from(self.const_eval(index)?).ok()?;
                let element = node.canonical_type()?.to_owned();
                let size = self.size_of(&element)?;
                Some(base_offset + index * size)
            }
            _ => None,
        }
    }

    /// The value of the catalog's `offsetof`: `__builtin_offsetof(t, d) +
    /// 0 * sizeof(&((t *)0)->d)`.
    fn offsetof_value(&mut self, lhs: &Node, rhs: &Node) -> Option<u64> {
        let member = self.offsetof_member(lhs, rhs)?;
        self.member_offset(member)
    }

    /// The member access the catalog's `offsetof` writes beside it,
    /// `&((t *)0)->d`.
    pub(crate) fn offsetof_member<'n>(&mut self, lhs: &Node, rhs: &'n Node) -> Option<&'n Node> {
        if strip_casts(lhs)?.kind != Kind::OffsetOfExpr {
            return None;
        }
        let rhs = strip_casts(rhs)?;
        if rhs.kind != Kind::BinaryOperator || rhs.opcode.as_deref() != Some("*") {
            return None;
        }
        let [zero, size] = rhs.inner.as_slice() else {
            return None;
        };
        if self.const_eval(zero)? != 0 {
            return None;
        }
        let size = strip_casts(size)?;
        if size.kind != Kind::UnaryExprOrTypeTraitExpr || size.name.as_deref() != Some("sizeof") {
            return None;
        }
        size.inner.first()
    }

    /// An integer constant expression's value.
    fn const_eval(&mut self, node: &Node) -> Option<i128> {
        match &node.kind {
            Kind::ConstantExpr => match &node.value {
                Some(Literal::Text(text)) => text.parse().ok(),
                Some(Literal::Number(n)) => Some(i128::from(*n)),
                None => self.const_eval(node.inner.first()?),
            },
            Kind::IntegerLiteral => match &node.value {
                Some(Literal::Text(text)) => text.parse().ok(),
                Some(Literal::Number(n)) => Some(i128::from(*n)),
                None => None,
            },
            Kind::CharacterLiteral => match &node.value {
                Some(Literal::Number(n)) => Some(i128::from(*n)),
                _ => None,
            },
            Kind::DeclRefExpr => {
                let referenced = node.referenced.as_ref()?;
                self.enum_values.get(&referenced.id).copied()
            }
            Kind::ParenExpr | Kind::ImplicitCastExpr | Kind::CStyleCastExpr => {
                let value = self.const_eval(node.inner.iter().find(|c| is_expression(&c.kind))?)?;
                Some(convert(
                    value,
                    node.canonical_type().unwrap_or(""),
                    self.platform,
                ))
            }
            Kind::UnaryOperator => {
                let value = self.const_eval(node.inner.first()?)?;
                match node.opcode.as_deref()? {
                    "-" => Some(value.wrapping_neg()),
                    "+" => Some(value),
                    "~" => Some(convert(
                        !value,
                        node.canonical_type().unwrap_or(""),
                        self.platform,
                    )),
                    "!" => Some(i128::from(value == 0)),
                    _ => None,
                }
            }
            Kind::UnaryExprOrTypeTraitExpr => {
                let of = match &node.arg_type {
                    Some(arg) => arg.to_string(),
                    None => node.inner.first()?.canonical_type()?.to_owned(),
                };
                let value = match node.name.as_deref()? {
                    "sizeof" => self.size_of(&of)?,
                    "alignof" | "_Alignof" | "__alignof" | "__alignof__" => self.align_of(&of)?,
                    _ => return None,
                };
                Some(i128::from(value))
            }
            Kind::BinaryOperator => {
                let [lhs, rhs] = node.inner.as_slice() else {
                    return None;
                };
                let opcode = node.opcode.as_deref()?;
                if opcode == "+"
                    && let Some(offset) = self.offsetof_value(lhs, rhs)
                {
                    return Some(i128::from(offset));
                }
                let left = self.const_eval(lhs)?;
                if opcode == "&&" {
                    return Some(i128::from(left != 0 && self.const_eval(rhs)? != 0));
                }
                if opcode == "||" {
                    return Some(i128::from(left != 0 || self.const_eval(rhs)? != 0));
                }
                let right = self.const_eval(rhs)?;
                if opcode == "," {
                    return Some(right);
                }
                let op = BinOp::from_opcode(opcode)?;
                let ty = node.canonical_type().unwrap_or("").to_owned();
                int_binary(op, left, right, &ty, self.platform)
            }
            Kind::ConditionalOperator => {
                let [cond, yes, no] = node.inner.as_slice() else {
                    return None;
                };
                if self.const_eval(cond)? != 0 {
                    self.const_eval(yes)
                } else {
                    self.const_eval(no)
                }
            }
            _ => None,
        }
    }
}

/// An integer type's width in bits and signedness, by the target; `None`
/// for any other type.
fn integer_type(ty: &str, platform: Platform) -> Option<(u32, bool)> {
    let long = match platform {
        Platform::Linux => 64,
        Platform::Windows => 32,
    };
    Some(match strip_qualifiers(ty) {
        "_Bool" | "bool" => (1, false),
        "char" | "signed char" => (8, true),
        "unsigned char" => (8, false),
        "short" | "signed short" => (16, true),
        "unsigned short" => (16, false),
        "int" | "signed int" => (32, true),
        "unsigned int" => (32, false),
        "long" | "signed long" => (long, true),
        "unsigned long" => (long, false),
        "long long" | "signed long long" => (64, true),
        "unsigned long long" => (64, false),
        "__int128" => (128, true),
        "unsigned __int128" => (128, false),
        _ => return None,
    })
}

/// A value converted to an integer type, by its width and signedness; as
/// it is for any other type.
fn convert(value: i128, ty: &str, platform: Platform) -> i128 {
    let Some((bits, signed)) = integer_type(ty, platform) else {
        return value;
    };
    if bits == 1 {
        return i128::from(value != 0);
    }
    if bits == 128 {
        return value;
    }
    let mask = (1i128 << bits) - 1;
    let low = value & mask;
    if signed && (low >> (bits - 1)) & 1 == 1 {
        low - (1i128 << bits)
    } else {
        low
    }
}

/// An integer operation on two values, in the result's type; `None` where
/// C leaves it undefined.
fn int_binary(op: BinOp, left: i128, right: i128, ty: &str, platform: Platform) -> Option<i128> {
    let value = match op {
        BinOp::Add => left.wrapping_add(right),
        BinOp::Sub => left.wrapping_sub(right),
        BinOp::Mul => left.wrapping_mul(right),
        BinOp::Div => left.checked_div(right)?,
        BinOp::Rem => left.checked_rem(right)?,
        BinOp::Shl => left.checked_shl(u32::try_from(right).ok()?)?,
        BinOp::Shr => left.checked_shr(u32::try_from(right).ok()?)?,
        BinOp::BitAnd => left & right,
        BinOp::BitOr => left | right,
        BinOp::BitXor => left ^ right,
        BinOp::Eq => i128::from(left == right),
        BinOp::Ne => i128::from(left != right),
        BinOp::Lt => i128::from(left < right),
        BinOp::Le => i128::from(left <= right),
        BinOp::Gt => i128::from(left > right),
        BinOp::Ge => i128::from(left >= right),
    };
    Some(convert(value, ty, platform))
}

/// A path in the image, lexically resolved, its source-tree files named
/// from the tree's top.
pub(crate) fn catalog_path(root: &str, file: &str) -> Arc<str> {
    // A path in the source tree as the catalog already names it.
    if !file.starts_with('/') {
        return Arc::from(file);
    }
    let inside = file.strip_prefix(root).unwrap_or(file);
    let mut parts: Vec<&str> = Vec::new();
    for part in inside.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    let path = format!("/{}", parts.join("/"));
    match path.strip_prefix(SOURCE_TREE) {
        Some(rest) => Arc::from(rest),
        None => Arc::from(path),
    }
}

/// A type's text with each unnamed record's place (`at <path>:<line>:<col>`)
/// named as the catalog names files.
pub(crate) fn clean_type(root: &str, text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(" at ") {
        let start = at + 4;
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let end = tail.find(')').unwrap_or(tail.len());
        let place = &tail[..end];
        let mut pieces = place.rsplitn(3, ':');
        match (pieces.next(), pieces.next(), pieces.next()) {
            (Some(col), Some(line), Some(path)) => {
                out.push_str(&catalog_path(root, path));
                out.push(':');
                out.push_str(line);
                out.push(':');
                out.push_str(col);
            }
            _ => out.push_str(place),
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

/// A type's text without its outermost qualifiers.
fn strip_qualifiers(text: &str) -> &str {
    let mut text = text.trim();
    loop {
        let before = text;
        for prefix in [
            "const ",
            "volatile ",
            "restrict ",
            "__restrict ",
            "_Atomic ",
        ] {
            text = text.strip_prefix(prefix).unwrap_or(text).trim_start();
        }
        // A qualifier after the declarator's `*` too (`char *const`).
        for suffix in ["const", "volatile", "restrict", "__restrict"] {
            if let Some(rest) = text.strip_suffix(suffix)
                && (rest.ends_with(' ') || rest.ends_with('*'))
            {
                text = rest.trim_end();
            }
        }
        if text == before {
            return text;
        }
    }
}

/// Where the parenthesis opened at `open` closes.
fn matching_paren(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0;
    for (i, c) in text[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + i);
                }
            }
            _ => {}
        }
    }
    None
}

/// A record's member.
#[derive(Debug, Clone)]
pub(crate) struct Member {
    /// Its name; an anonymous member's is its record's.
    pub(crate) name: Arc<str>,
    /// The record its type names, where the record defining it defines
    /// that record too.
    pub(crate) record: Option<Arc<str>>,
    pub(crate) anonymous: bool,
}

/// The `count`th record of a name: the first by the name alone, the next
/// by the name and its number.
pub(crate) fn numbered(name: Arc<str>, count: u32) -> Arc<str> {
    if count <= 1 {
        name
    } else {
        Arc::from(format!("{name}#{count}"))
    }
}

/// A record's name without the number telling it from an earlier one of
/// the name.
fn unnumbered(name: &str) -> &str {
    match name.rsplit_once('#') {
        Some((base, number)) if number.bytes().all(|b| b.is_ascii_digit()) => base,
        _ => name,
    }
}

/// An array type's element type: the type without its first bound
/// (`T [3]` for `T [2][3]`).
fn array_element(text: &str) -> String {
    let text = text.trim_end();
    let Some(open) = text.find('[') else {
        return text.to_owned();
    };
    let close = text[open..].find(']').map_or(text.len(), |c| open + c + 1);
    let rest = text[close..].trim_start();
    if rest.is_empty() {
        text[..open].trim_end().to_owned()
    } else {
        format!("{} {rest}", text[..open].trim_end())
    }
}

/// The bounds of a declarator's arrays (`[3][4]`), none where it has none;
/// `None` where one is not a number.
fn array_bounds(text: &str) -> Option<Vec<u64>> {
    let mut bounds = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let close = rest[open..].find(']')? + open;
        bounds.push(rest[open + 1..close].trim().parse().ok()?);
        rest = &rest[close + 1..];
    }
    Some(bounds)
}

/// The size and alignment of a scalar type, by the target.
fn scalar_size(text: &str, platform: Platform) -> Option<(u64, u64)> {
    let long = match platform {
        Platform::Linux => 8,
        Platform::Windows => 4,
    };
    let long_double = match platform {
        Platform::Linux => 16,
        Platform::Windows => 8,
    };
    let base = text
        .strip_suffix(" _Complex")
        .or_else(|| text.strip_prefix("_Complex "));
    if let Some(base) = base {
        let (size, align) = scalar_size(base, platform)?;
        return Some((size * 2, align));
    }
    let size = match text {
        "void" | "_Bool" | "bool" | "char" | "signed char" | "unsigned char" => 1,
        "short" | "unsigned short" | "_Float16" | "__bf16" => 2,
        "int" | "unsigned int" | "float" => 4,
        "long" | "unsigned long" => long,
        "long long" | "unsigned long long" | "double" => 8,
        "long double" => long_double,
        "__int128" | "unsigned __int128" | "_Float128" | "__float128" => 16,
        _ => return None,
    };
    Some((size, size))
}

/// An expression through its implicit casts and parentheses.
fn strip_casts(node: &Node) -> Option<&Node> {
    match node.kind {
        Kind::ImplicitCastExpr | Kind::ParenExpr => strip_casts(node.inner.first()?),
        _ => Some(node),
    }
}

/// A type's text without its qualifiers, pointers and array bounds.
fn strip_declarator(text: &str) -> &str {
    let mut text = text.trim();
    loop {
        let before = text;
        for prefix in ["const ", "volatile ", "restrict ", "__restrict "] {
            text = text.strip_prefix(prefix).unwrap_or(text).trim_start();
        }
        for suffix in [" const", " volatile", " restrict", " __restrict", "*"] {
            text = text.strip_suffix(suffix).unwrap_or(text).trim_end();
        }
        if text.ends_with(']')
            && let Some(open) = text.rfind('[')
        {
            text = text[..open].trim_end();
        }
        if text == before {
            return text;
        }
    }
}

/// The name of an unnamed record a type names (`struct (unnamed at
/// Zend/zend_types.h:330:2)`, or as a member of a named one).
pub(crate) fn anonymous_record(text: &str) -> Option<Arc<str>> {
    let open = text.find("(unnamed").or_else(|| text.find("(anonymous"))?;
    let at = text[open..].find(" at ")? + open + 4;
    let place = text[at..].trim_end_matches(')');
    Some(Arc::from(format!("anon@{place}")))
}

pub(crate) fn is_static(node: &Node) -> bool {
    node.storage_class.as_deref() == Some("static")
}

pub(crate) fn has_body(node: &Node) -> bool {
    node.inner
        .iter()
        .any(|child| child.kind == Kind::CompoundStmt)
}

/// Whether a function's declaration says it does not return.
pub(crate) fn is_noreturn(node: &Node) -> bool {
    node.inner.iter().any(|child| {
        matches!(&child.kind, Kind::Attr(name)
            if matches!(&**name, "NoReturnAttr" | "C11NoReturnAttr" | "CXX11NoReturnAttr"))
    }) || node.ty.as_deref().is_some_and(type_is_noreturn)
}

fn type_is_noreturn(ty: &str) -> bool {
    ty.contains("__attribute__((noreturn))")
}

/// Whether a node of this kind is an expression, not a declaration,
/// attribute, comment or type the dump lists beside one.
fn is_expression(kind: &Kind) -> bool {
    match kind {
        Kind::Attr(_) | Kind::Null => false,
        Kind::Other(name) => {
            !(name.ends_with("Comment") || name.ends_with("Type") || name.ends_with("Decl"))
        }
        _ => true,
    }
}

/// A variable's initializer: its last child that is an expression.
fn initializer(node: &Node) -> Option<&Node> {
    if !node.has_init {
        return None;
    }
    node.inner
        .iter()
        .rev()
        .find(|child| is_expression(&child.kind))
}

/// A C string literal's bytes, from the dump's spelling of it (quoted,
/// escaped); `None` for a wide literal.
pub(crate) fn unescape(spelled: &str) -> Option<Vec<u8>> {
    let body = spelled.strip_prefix("u8").unwrap_or(spelled);
    let body = body.strip_prefix('"')?.strip_suffix('"')?;
    let bytes = body.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' {
            // `""` splits a literal where a hexadecimal escape is followed
            // by a hexadecimal digit.
            i += 2;
            continue;
        }
        if b != b'\\' || i + 1 >= bytes.len() {
            out.push(b);
            i += 1;
            continue;
        }
        let e = bytes[i + 1];
        i += 2;
        match e {
            b'n' => out.push(b'\n'),
            b't' => out.push(b'\t'),
            b'r' => out.push(b'\r'),
            b'a' => out.push(7),
            b'b' => out.push(8),
            b'f' => out.push(12),
            b'v' => out.push(11),
            b'e' => out.push(27),
            b'0'..=b'7' => {
                let mut value = u32::from(e - b'0');
                let mut digits = 1;
                while digits < 3 && i < bytes.len() && (b'0'..=b'7').contains(&bytes[i]) {
                    value = value * 8 + u32::from(bytes[i] - b'0');
                    i += 1;
                    digits += 1;
                }
                out.push(value as u8);
            }
            b'x' => {
                let mut value = 0u32;
                while i < bytes.len() && bytes[i].is_ascii_hexdigit() {
                    value = value * 16 + char::from(bytes[i]).to_digit(16).unwrap_or(0);
                    i += 1;
                }
                out.push(value as u8);
            }
            other => out.push(other),
        }
    }
    Some(out)
}

/// Lowers a function definition; gives it and the static variables it
/// declares with an initializer.
pub(crate) fn lower_function(
    index: &mut TuIndex,
    node: &Node,
    name: FnName,
) -> (Function, Vec<Global>) {
    let file = index.node_path(node);
    let ret_text = node
        .ty
        .as_deref()
        .and_then(|ty| ty.split_once('(').map(|(ret, _)| ret.trim().to_owned()))
        .unwrap_or_else(|| "void".to_owned());
    let ret = index.ty(&ret_text);
    let mut lowerer = Lowerer::new(index);
    let mut params = 0;
    for child in &node.inner {
        if child.kind == Kind::ParmVarDecl {
            lowerer.declare(child);
            params += 1;
        }
    }
    if let Some(body) = node.inner.iter().find(|c| c.kind == Kind::CompoundStmt) {
        lowerer.stmt(body);
    }
    let end = if ret_text == "void" {
        Term::Return(None)
    } else {
        Term::Unreachable
    };
    lowerer.set_term(end);
    lowerer.resolve_indirect_gotos();
    let mut function = Function {
        name,
        file,
        line: node.loc.line,
        is_variadic: node.is_variadic,
        params,
        vars: lowerer.vars,
        ret,
        blocks: lowerer.blocks,
        unread: lowerer.unread,
        approximated: lowerer.approximated,
    };
    crate::cfg::simplify(&mut function);
    (function, lowerer.statics)
}

/// Lowers a global variable and its initializer.
pub(crate) fn lower_global(index: &mut TuIndex, node: &Node, name: GlobalName) -> Global {
    let ty = index.node_ty(node);
    let init = initializer(node).map(|init| {
        let mut lowerer = Lowerer::new(index);
        lowerer.init(init)
    });
    Global { name, ty, init }
}

struct SwitchContext {
    cases: Vec<(i128, i128, BlockId)>,
    default: Option<BlockId>,
}

struct Lowerer<'i> {
    index: &'i mut TuIndex,
    vars: Vec<Var>,
    locals: FxHashMap<u64, VarId>,
    /// Static and `extern` variables declared in the function, as globals.
    block_globals: FxHashMap<u64, GlobalName>,
    /// The static variables declared with an initializer.
    statics: Vec<Global>,
    blocks: Vec<Block>,
    current: BlockId,
    labels: FxHashMap<u64, BlockId>,
    /// The blocks whose address is taken, which a computed `goto` reaches.
    address_taken: Vec<BlockId>,
    breaks: Vec<BlockId>,
    continues: Vec<BlockId>,
    switches: Vec<SwitchContext>,
    unread: Vec<Arc<str>>,
    approximated: Vec<Arc<str>>,
}

impl<'i> Lowerer<'i> {
    fn new(index: &'i mut TuIndex) -> Lowerer<'i> {
        Lowerer {
            index,
            vars: Vec::new(),
            locals: FxHashMap::default(),
            block_globals: FxHashMap::default(),
            statics: Vec::new(),
            blocks: vec![Block {
                stmts: Vec::new(),
                term: Term::Unreachable,
            }],
            current: 0,
            labels: FxHashMap::default(),
            address_taken: Vec::new(),
            breaks: Vec::new(),
            continues: Vec::new(),
            switches: Vec::new(),
            unread: Vec::new(),
            approximated: Vec::new(),
        }
    }

    fn not_read(&mut self, what: impl Into<String>) {
        let what: Arc<str> = Arc::from(what.into());
        if !self.unread.contains(&what) {
            self.unread.push(what);
        }
    }

    fn approximate(&mut self, what: impl Into<String>) {
        let what: Arc<str> = Arc::from(what.into());
        if !self.approximated.contains(&what) {
            self.approximated.push(what);
        }
    }

    fn ty(&mut self, node: &Node) -> Ty {
        self.index.node_ty(node)
    }

    fn declare(&mut self, node: &Node) -> VarId {
        let ty = self.ty(node);
        let id = self.vars.len() as VarId;
        self.vars.push(Var {
            name: Arc::from(node.name.as_deref().unwrap_or("")),
            ty,
        });
        self.locals.insert(node.id, id);
        id
    }

    fn temp(&mut self, ty: Ty) -> VarId {
        let id = self.vars.len() as VarId;
        self.vars.push(Var {
            name: Arc::from(format!("${id}")),
            ty,
        });
        id
    }

    fn new_block(&mut self) -> BlockId {
        self.blocks.push(Block {
            stmts: Vec::new(),
            term: Term::Unreachable,
        });
        (self.blocks.len() - 1) as BlockId
    }

    fn push(&mut self, stmt: Stmt) {
        self.blocks[self.current as usize].stmts.push(stmt);
    }

    fn set_term(&mut self, term: Term) {
        self.blocks[self.current as usize].term = term;
    }

    /// Ends the current block with `term` and goes on in a new block, which
    /// only a label or a case reaches.
    fn terminate(&mut self, term: Term) {
        self.set_term(term);
        self.current = self.new_block();
    }

    /// Ends the current block with a jump to `target`, then goes on there.
    fn continue_at(&mut self, target: BlockId) {
        self.set_term(Term::Goto(target));
        self.current = target;
    }

    fn label_block(&mut self, id: u64) -> BlockId {
        if let Some(&block) = self.labels.get(&id) {
            return block;
        }
        let block = self.new_block();
        self.labels.insert(id, block);
        block
    }

    fn resolve_indirect_gotos(&mut self) {
        let taken = self.address_taken.clone();
        for block in &mut self.blocks {
            if let Term::IndirectGoto(targets) = &mut block.term {
                targets.clone_from(&taken);
            }
        }
    }

    fn assign(&mut self, dst: Place, src: Expr, line: u32) {
        self.push(Stmt::Assign { dst, src, line });
    }

    /// An integer operation or conversion on constants, as its value.
    fn fold(&self, expr: Expr) -> Expr {
        if let ExprKind::Offset(pointer, bytes) = &expr.kind
            && let ExprKind::Int(outer) = bytes.kind
        {
            if let ExprKind::Offset(inner, inner_bytes) = &pointer.kind
                && let ExprKind::Int(first) = inner_bytes.kind
            {
                let bytes = Expr {
                    kind: ExprKind::Int(first.wrapping_add(outer)),
                    ty: bytes.ty.clone(),
                };
                return Expr {
                    kind: ExprKind::Offset(inner.clone(), Box::new(bytes)),
                    ty: expr.ty,
                };
            }
            if outer == 0 && pointer.ty == expr.ty {
                return (**pointer).clone();
            }
            return expr;
        }
        let platform = self.index.platform;
        if integer_type(&expr.ty, platform).is_none() {
            return expr;
        }
        let value = match &expr.kind {
            ExprKind::Binary(op, left, right) => match (&left.kind, &right.kind) {
                (ExprKind::Int(left), ExprKind::Int(right)) => {
                    int_binary(*op, *left, *right, &expr.ty, platform)
                }
                _ => None,
            },
            ExprKind::Unary(op, operand) => match operand.kind {
                ExprKind::Int(value) => Some(match op {
                    UnOp::Neg => convert(value.wrapping_neg(), &expr.ty, platform),
                    UnOp::Not => i128::from(value == 0),
                    UnOp::BitNot => convert(!value, &expr.ty, platform),
                }),
                _ => None,
            },
            ExprKind::Cast(operand) => match operand.kind {
                ExprKind::Int(value) => Some(convert(value, &expr.ty, platform)),
                _ => None,
            },
            _ => None,
        };
        match value {
            Some(value) => Expr {
                kind: ExprKind::Int(value),
                ty: expr.ty,
            },
            None => expr,
        }
    }

    fn int(&mut self, value: i128) -> Expr {
        Expr {
            kind: ExprKind::Int(value),
            ty: self.index.ty("int"),
        }
    }

    /// An index of elements of `size` bytes, in bytes.
    fn bytes(&mut self, index: Expr, size: Option<u64>) -> Expr {
        let ty = self.index.ty("long");
        let Some(size) = size else {
            self.approximate("an element of unknown size");
            return Expr {
                kind: ExprKind::Unknown(Arc::from("element size")),
                ty,
            };
        };
        let size = Expr {
            kind: ExprKind::Int(i128::from(size)),
            ty: ty.clone(),
        };
        let index = self.fold(Expr {
            kind: ExprKind::Cast(Box::new(index)),
            ty: ty.clone(),
        });
        self.fold(Expr {
            kind: ExprKind::Binary(BinOp::Mul, Box::new(index), Box::new(size)),
            ty,
        })
    }

    /// `pointer` moved by `index` of what it points to, back where
    /// `backwards`; `pointer_ty` is its type as the dump writes it.
    fn pointer_offset(
        &mut self,
        pointer: Expr,
        index: Expr,
        backwards: bool,
        pointer_ty: &str,
        ty: Ty,
    ) -> Expr {
        let size = self.index.pointee_size(pointer_ty);
        let index = if backwards {
            let ty = index.ty.clone();
            self.fold(Expr {
                kind: ExprKind::Unary(UnOp::Neg, Box::new(index)),
                ty,
            })
        } else {
            index
        };
        let bytes = self.bytes(index, size);
        self.fold(Expr {
            kind: ExprKind::Offset(Box::new(pointer), Box::new(bytes)),
            ty,
        })
    }

    fn unknown(&mut self, node: &Node, why: &str) -> Expr {
        Expr {
            kind: ExprKind::Unknown(Arc::from(why)),
            ty: self.ty(node),
        }
    }

    fn use_var(id: VarId, ty: Ty) -> Expr {
        Expr {
            kind: ExprKind::Use(Place::Var(id)),
            ty,
        }
    }

    // Statements.

    fn stmt(&mut self, node: &Node) {
        let line = node.loc.line;
        match &node.kind {
            Kind::CompoundStmt => {
                for child in &node.inner {
                    self.stmt(child);
                }
            }
            Kind::DeclStmt => {
                for child in &node.inner {
                    match child.kind {
                        Kind::VarDecl => self.local(child),
                        _ => self.index.index_type(child),
                    }
                }
            }
            Kind::IfStmt => {
                let (cond, then, els) = match node.inner.as_slice() {
                    [cond, then] => (cond, then, None),
                    [cond, then, els] => (cond, then, Some(els)),
                    _ => {
                        self.not_read("if statement shape");
                        return;
                    }
                };
                let then_block = self.new_block();
                let join = self.new_block();
                let else_block = if els.is_some() {
                    self.new_block()
                } else {
                    join
                };
                self.cond(cond, then_block, else_block);
                self.current = then_block;
                self.stmt(then);
                self.set_term(Term::Goto(join));
                if let Some(els) = els {
                    self.current = else_block;
                    self.stmt(els);
                    self.set_term(Term::Goto(join));
                }
                self.current = join;
            }
            Kind::WhileStmt => {
                let [cond, body] = node.inner.as_slice() else {
                    self.not_read("while statement shape");
                    return;
                };
                let head = self.new_block();
                let body_block = self.new_block();
                let exit = self.new_block();
                self.continue_at(head);
                self.cond(cond, body_block, exit);
                self.current = body_block;
                self.loop_body(body, exit, head);
                self.set_term(Term::Goto(head));
                self.current = exit;
            }
            Kind::DoStmt => {
                let [body, cond] = node.inner.as_slice() else {
                    self.not_read("do statement shape");
                    return;
                };
                let body_block = self.new_block();
                let cond_block = self.new_block();
                let exit = self.new_block();
                self.continue_at(body_block);
                self.loop_body(body, exit, cond_block);
                self.continue_at(cond_block);
                self.cond(cond, body_block, exit);
                self.current = exit;
            }
            Kind::ForStmt => {
                let [init, _, cond, inc, body] = node.inner.as_slice() else {
                    self.not_read("for statement shape");
                    return;
                };
                if init.kind != Kind::Null {
                    self.stmt(init);
                }
                let head = self.new_block();
                let body_block = self.new_block();
                let inc_block = self.new_block();
                let exit = self.new_block();
                self.continue_at(head);
                if cond.kind == Kind::Null {
                    self.set_term(Term::Goto(body_block));
                } else {
                    self.cond(cond, body_block, exit);
                }
                self.current = body_block;
                self.loop_body(body, exit, inc_block);
                self.continue_at(inc_block);
                if inc.kind != Kind::Null {
                    self.discard(inc);
                }
                self.set_term(Term::Goto(head));
                self.current = exit;
            }
            Kind::SwitchStmt => {
                let [cond, body] = node.inner.as_slice() else {
                    self.not_read("switch statement shape");
                    return;
                };
                let on = self.expr(cond);
                let head = self.current;
                let exit = self.new_block();
                self.current = self.new_block();
                self.breaks.push(exit);
                self.switches.push(SwitchContext {
                    cases: Vec::new(),
                    default: None,
                });
                self.stmt(body);
                self.breaks.pop();
                let context = self.switches.pop().expect("pushed above");
                self.set_term(Term::Goto(exit));
                self.blocks[head as usize].term = Term::Switch {
                    on,
                    cases: context.cases,
                    default: context.default.unwrap_or(exit),
                };
                self.current = exit;
            }
            Kind::CaseStmt => {
                let bounds = if node.is_gnu_range { 2 } else { 1 };
                let low = node.inner.first().and_then(|c| self.index.const_eval(c));
                let high = if node.is_gnu_range {
                    node.inner.get(1).and_then(|c| self.index.const_eval(c))
                } else {
                    low
                };
                let block = self.new_block();
                self.continue_at(block);
                match (low, high) {
                    (Some(low), Some(high)) => {
                        if let Some(context) = self.switches.last_mut() {
                            context.cases.push((low, high, block));
                        }
                    }
                    _ => self.not_read("case label value"),
                }
                for child in node.inner.iter().skip(bounds) {
                    self.stmt(child);
                }
            }
            Kind::DefaultStmt => {
                let block = self.new_block();
                self.continue_at(block);
                if let Some(context) = self.switches.last_mut() {
                    context.default = Some(block);
                }
                for child in &node.inner {
                    self.stmt(child);
                }
            }
            Kind::BreakStmt => match self.breaks.last().copied() {
                Some(target) => self.terminate(Term::Goto(target)),
                None => self.not_read("break outside a loop"),
            },
            Kind::ContinueStmt => match self.continues.last().copied() {
                Some(target) => self.terminate(Term::Goto(target)),
                None => self.not_read("continue outside a loop"),
            },
            Kind::ReturnStmt => {
                let value = node.inner.first().map(|e| self.expr(e));
                self.terminate(Term::Return(value));
            }
            Kind::GotoStmt => match node.target_label {
                Some(id) => {
                    let target = self.label_block(id);
                    self.terminate(Term::Goto(target));
                }
                None => self.not_read("goto without a label"),
            },
            Kind::IndirectGotoStmt => {
                if let Some(target) = node.inner.first() {
                    self.discard(target);
                }
                self.terminate(Term::IndirectGoto(Vec::new()));
            }
            Kind::LabelStmt => {
                let block = match node.decl_id {
                    Some(id) => self.label_block(id),
                    None => self.new_block(),
                };
                self.continue_at(block);
                for child in &node.inner {
                    self.stmt(child);
                }
            }
            // A statement's attributes (`unused` on a label, `fallthrough`)
            // change nothing it does.
            Kind::NullStmt | Kind::Null | Kind::Attr(_) => {}
            Kind::AttributedStmt => {
                for child in &node.inner {
                    if !matches!(child.kind, Kind::Attr(_)) {
                        self.stmt(child);
                    }
                }
            }
            Kind::GccAsmStmt => {
                let mut writes = Vec::new();
                for operand in &node.inner {
                    if operand.is_lvalue {
                        writes.push(self.place(operand));
                    } else {
                        self.discard(operand);
                    }
                }
                self.push(Stmt::Asm { writes, line });
            }
            Kind::Other(name) if !(name.ends_with("Expr") || name.ends_with("Operator")) => {
                let name = name.to_string();
                self.approximate(name);
                for child in &node.inner {
                    self.stmt(child);
                }
            }
            kind if is_expression(kind) => self.discard(node),
            other => self.not_read(format!("{other:?} as a statement")),
        }
    }

    fn loop_body(&mut self, body: &Node, exit: BlockId, next: BlockId) {
        self.breaks.push(exit);
        self.continues.push(next);
        self.stmt(body);
        self.breaks.pop();
        self.continues.pop();
    }

    fn local(&mut self, node: &Node) {
        let name = node.name.as_deref().unwrap_or("");
        match node.storage_class.as_deref() {
            Some("extern") => {
                self.block_globals.insert(node.id, Arc::from(name));
                return;
            }
            Some("static") => {
                let path = self.index.node_path(node);
                let global: GlobalName = Arc::from(format!("{path}:{name}@{}", node.loc.offset));
                self.block_globals.insert(node.id, global.clone());
                if let Some(init) = initializer(node) {
                    let ty = self.ty(node);
                    let init = self.init(init);
                    self.statics.push(Global {
                        name: global,
                        ty,
                        init: Some(init),
                    });
                }
                return;
            }
            _ => {}
        }
        let id = self.declare(node);
        if let Some(init) = initializer(node) {
            self.initialize(Place::Var(id), init);
        }
    }

    /// Assigns an initializer to `place`, an initializer list member by
    /// member.
    fn initialize(&mut self, place: Place, init: &Node) {
        self.initialize_record(place, init, None);
    }

    /// Assigns an initializer to `place`, whose record, or its elements',
    /// is `record` where the member holding it tells.
    fn initialize_record(&mut self, place: Place, init: &Node, record: Option<Arc<str>>) {
        let line = init.loc.line;
        match init.kind {
            Kind::InitListExpr => {
                let ty = init.canonical_type().unwrap_or("").to_owned();
                if ty.trim_end().ends_with(']') {
                    let size = self.index.size_of(&array_element(&ty));
                    for (i, item) in init.inner.iter().enumerate() {
                        let index = self.int(i as i128);
                        let bytes = self.bytes(index, size);
                        let element = Place::Index(Box::new(place.clone()), Box::new(bytes));
                        self.initialize_record(element, item, record.clone());
                    }
                    return;
                }
                let record = record.unwrap_or_else(|| self.index.record_of(&ty));
                let members = self.index.record_members(&record);
                if let Some(field) = &init.field {
                    let (record, name) = self.index.member(field.id).unwrap_or_else(|| {
                        (record, Arc::from(field.name.as_deref().unwrap_or("")))
                    });
                    let held = members
                        .iter()
                        .find(|m| m.name == name)
                        .and_then(|m| m.record.clone());
                    let field = self.index.field(record, name);
                    if let Some(value) = init.inner.first() {
                        self.initialize_record(Place::Field(Box::new(place), field), value, held);
                    }
                    return;
                }
                for (i, item) in init.inner.iter().enumerate() {
                    let member = members.get(i);
                    let name =
                        member.map_or_else(|| Arc::from(format!("#{i}")), |m| m.name.clone());
                    let held = member.and_then(|m| m.record.clone());
                    let field = self.index.field(record.clone(), name);
                    self.initialize_record(
                        Place::Field(Box::new(place.clone()), field),
                        item,
                        held,
                    );
                }
            }
            Kind::ImplicitValueInitExpr => {
                let zero = self.int(0);
                self.assign(place, zero, line);
            }
            _ => {
                let value = self.expr(init);
                self.assign(place, value, line);
            }
        }
    }

    // Conditions.

    /// Lowers `node` as a condition: to `yes` where it holds, to `no` where
    /// not.
    fn cond(&mut self, node: &Node, yes: BlockId, no: BlockId) {
        match &node.kind {
            Kind::ParenExpr | Kind::ExprWithCleanups if !node.inner.is_empty() => {
                return self.cond(&node.inner[0], yes, no);
            }
            Kind::UnaryOperator
                if node.opcode.as_deref() == Some("!") && !node.inner.is_empty() =>
            {
                return self.cond(&node.inner[0], no, yes);
            }
            Kind::BinaryOperator if node.inner.len() == 2 => match node.opcode.as_deref() {
                Some("&&") => {
                    let mid = self.new_block();
                    self.cond(&node.inner[0], mid, no);
                    self.current = mid;
                    return self.cond(&node.inner[1], yes, no);
                }
                Some("||") => {
                    let mid = self.new_block();
                    self.cond(&node.inner[0], yes, mid);
                    self.current = mid;
                    return self.cond(&node.inner[1], yes, no);
                }
                Some(",") => {
                    self.discard(&node.inner[0]);
                    return self.cond(&node.inner[1], yes, no);
                }
                _ => {}
            },
            Kind::ConditionalOperator if node.inner.len() == 3 => {
                let on_true = self.new_block();
                let on_false = self.new_block();
                self.cond(&node.inner[0], on_true, on_false);
                self.current = on_true;
                self.cond(&node.inner[1], yes, no);
                self.current = on_false;
                return self.cond(&node.inner[2], yes, no);
            }
            Kind::ImplicitCastExpr | Kind::CStyleCastExpr
                if matches!(
                    node.cast_kind.as_deref(),
                    Some("IntegralToBoolean" | "PointerToBoolean" | "NoOp")
                ) && !node.inner.is_empty() =>
            {
                return self.cond(&node.inner[0], yes, no);
            }
            Kind::CallExpr => {
                if let Some((_, name)) = self.direct_callee(node)
                    && &*name == "__builtin_expect"
                    && node.inner.len() >= 2
                {
                    self.discard_all(&node.inner[2..]);
                    return self.cond(&node.inner[1], yes, no);
                }
            }
            _ => {}
        }
        if let Some(value) = self.index.const_eval(node) {
            let target = if value != 0 { yes } else { no };
            self.terminate(Term::Goto(target));
            return;
        }
        let cond = self.expr(node);
        self.terminate(Term::Branch {
            cond,
            then: yes,
            els: no,
        });
    }

    // Expressions.

    /// Lowers an expression for its effects alone.
    fn discard(&mut self, node: &Node) {
        match &node.kind {
            Kind::ImplicitCastExpr
            | Kind::CStyleCastExpr
            | Kind::ParenExpr
            | Kind::ExprWithCleanups
            | Kind::MaterializeTemporaryExpr
                if node.inner.len() == 1 =>
            {
                self.discard(&node.inner[0]);
            }
            Kind::BinaryOperator if node.inner.len() == 2 => match node.opcode.as_deref() {
                Some(",") => {
                    self.discard(&node.inner[0]);
                    self.discard(&node.inner[1]);
                }
                Some("&&" | "||") => {
                    let join = self.new_block();
                    self.cond(node, join, join);
                    self.current = join;
                }
                _ => {
                    self.expr(node);
                }
            },
            Kind::ConditionalOperator if node.inner.len() == 3 => {
                let on_true = self.new_block();
                let on_false = self.new_block();
                let join = self.new_block();
                self.cond(&node.inner[0], on_true, on_false);
                for (block, arm) in [(on_true, &node.inner[1]), (on_false, &node.inner[2])] {
                    self.current = block;
                    self.discard(arm);
                    self.set_term(Term::Goto(join));
                }
                self.current = join;
            }
            Kind::CallExpr => {
                self.call(node, false);
            }
            Kind::DeclRefExpr
            | Kind::IntegerLiteral
            | Kind::CharacterLiteral
            | Kind::FloatingLiteral
            | Kind::StringLiteral
            | Kind::UnaryExprOrTypeTraitExpr
            | Kind::OffsetOfExpr => {}
            _ => {
                self.expr(node);
            }
        }
    }

    fn discard_all(&mut self, nodes: &[Node]) {
        for node in nodes {
            self.discard(node);
        }
    }

    /// The function a call names directly: its declaration and its name.
    fn direct_callee(&self, call: &Node) -> Option<(u64, FnName)> {
        let decl = strip_to_decl_ref(call.inner.first()?)?;
        let referenced = decl.referenced.as_ref()?;
        if &*referenced.kind != "FunctionDecl" {
            return None;
        }
        let name = self
            .index
            .functions
            .get(&referenced.id)
            .cloned()
            .unwrap_or_else(|| Arc::from(referenced.name.as_deref().unwrap_or("")));
        Some((referenced.id, name))
    }

    /// Lowers a call; gives its value where `want_value` and the callee
    /// returns one.
    fn call(&mut self, node: &Node, want_value: bool) -> Option<Expr> {
        let Some(callee_node) = node.inner.first() else {
            self.not_read("call without a callee");
            return None;
        };
        let args = &node.inner[1..];
        let ty = self.ty(node);
        let line = node.loc.line;
        let mut noreturn = callee_node.ty.as_deref().is_some_and(type_is_noreturn);
        let callee = match self.direct_callee(node) {
            Some((id, name)) => {
                noreturn |= self.index.noreturn.contains(&id);
                match &*name {
                    "__builtin_expect" | "__builtin_expect_with_probability"
                        if !args.is_empty() =>
                    {
                        let value = self.expr(&args[0]);
                        self.discard_all(&args[1..]);
                        return Some(value);
                    }
                    "__builtin_unreachable" | "__builtin_trap" => {
                        self.discard_all(args);
                        self.terminate(Term::Unreachable);
                        return None;
                    }
                    "__builtin_assume" if args.len() == 1 => {
                        let holds = self.new_block();
                        let fails = self.new_block();
                        self.cond(&args[0], holds, fails);
                        self.blocks[fails as usize].term = Term::Unreachable;
                        self.current = holds;
                        return None;
                    }
                    "__builtin_constant_p" => {
                        return Some(Expr {
                            kind: ExprKind::Int(0),
                            ty,
                        });
                    }
                    _ => {}
                }
                Callee::Direct(name)
            }
            None => Callee::Indirect(self.callee_value(callee_node)),
        };
        let args: Vec<Expr> = args.iter().map(|arg| self.expr(arg)).collect();
        let dst = (want_value && &*ty != "void").then(|| self.temp(ty.clone()));
        self.push(Stmt::Call {
            dst: dst.map(Place::Var),
            callee,
            args,
            line,
        });
        if noreturn {
            self.terminate(Term::Unreachable);
        }
        dst.map(|id| Self::use_var(id, ty))
    }

    /// The function pointer a call goes through, `(*fp)` read as `fp`.
    fn callee_value(&mut self, callee: &Node) -> Expr {
        if callee.kind == Kind::ImplicitCastExpr
            && callee.cast_kind.as_deref() == Some("FunctionToPointerDecay")
            && let Some(inner) = callee.inner.first()
            && let Some(operand) = strip_parens(inner)
            && operand.kind == Kind::UnaryOperator
            && operand.opcode.as_deref() == Some("*")
            && let Some(pointer) = operand.inner.first()
        {
            return self.expr(pointer);
        }
        self.expr(callee)
    }

    fn expr(&mut self, node: &Node) -> Expr {
        let ty = self.ty(node);
        let line = node.loc.line;
        match &node.kind {
            Kind::ImplicitCastExpr | Kind::CStyleCastExpr => {
                let Some(inner) = node.inner.iter().find(|c| is_expression(&c.kind)) else {
                    return self.unknown(node, "cast without an operand");
                };
                match node.cast_kind.as_deref() {
                    Some("LValueToRValue") => {
                        let place = self.place(inner);
                        Expr {
                            kind: ExprKind::Use(place),
                            ty,
                        }
                    }
                    Some("ArrayToPointerDecay") => {
                        if let Some(bytes) = string_literal(inner) {
                            return Expr {
                                kind: ExprKind::Str(Arc::from(bytes)),
                                ty,
                            };
                        }
                        let place = self.place(inner);
                        let zero = self.int(0);
                        Expr {
                            kind: ExprKind::AddrOf(Place::Index(Box::new(place), Box::new(zero))),
                            ty,
                        }
                    }
                    Some("FunctionToPointerDecay" | "BuiltinFnToFnPtr") => {
                        let mut value = self.expr(inner);
                        value.ty = ty;
                        value
                    }
                    Some("ToVoid") => {
                        self.discard(inner);
                        Expr {
                            kind: ExprKind::Int(0),
                            ty,
                        }
                    }
                    _ => {
                        let value = self.expr(inner);
                        if value.ty == ty {
                            value
                        } else {
                            self.fold(Expr {
                                kind: ExprKind::Cast(Box::new(value)),
                                ty,
                            })
                        }
                    }
                }
            }
            // A temporary C materializes is the value it holds; its
            // cleanups are C++'s alone.
            Kind::ParenExpr | Kind::ExprWithCleanups | Kind::MaterializeTemporaryExpr => {
                match node.inner.first() {
                    Some(inner) => self.expr(inner),
                    None => self.unknown(node, "empty parentheses"),
                }
            }
            Kind::BuiltinBitCastExpr => {
                let Some(inner) = node.inner.iter().find(|c| is_expression(&c.kind)) else {
                    return self.unknown(node, "bit cast without an operand");
                };
                let value = self.expr(inner);
                Expr {
                    kind: ExprKind::Cast(Box::new(value)),
                    ty,
                }
            }
            Kind::TypeTraitExpr => match types_compatible(&mut *self.index, node) {
                Some(compatible) => Expr {
                    kind: ExprKind::Int(i128::from(compatible)),
                    ty,
                },
                None => {
                    self.approximate("TypeTraitExpr");
                    self.unknown(node, "type trait")
                }
            },
            Kind::DeclRefExpr => {
                let Some(referenced) = node.referenced.as_ref() else {
                    return self.unknown(node, "reference without a declaration");
                };
                match &*referenced.kind {
                    "EnumConstantDecl" => match self.index.enum_values.get(&referenced.id) {
                        Some(&value) => Expr {
                            kind: ExprKind::Int(value),
                            ty,
                        },
                        None => {
                            self.not_read("enumerator without a value");
                            self.unknown(node, "enumerator")
                        }
                    },
                    "FunctionDecl" => {
                        let name = self
                            .index
                            .functions
                            .get(&referenced.id)
                            .cloned()
                            .unwrap_or_else(|| Arc::from(referenced.name.as_deref().unwrap_or("")));
                        Expr {
                            kind: ExprKind::Func(name),
                            ty,
                        }
                    }
                    _ => {
                        let place = self.place(node);
                        Expr {
                            kind: ExprKind::Use(place),
                            ty,
                        }
                    }
                }
            }
            Kind::IntegerLiteral | Kind::CharacterLiteral | Kind::ConstantExpr => {
                match self.index.const_eval(node) {
                    Some(value) => Expr {
                        kind: ExprKind::Int(value),
                        ty,
                    },
                    None => match (&node.kind, node.inner.first()) {
                        (Kind::ConstantExpr, Some(inner)) => self.expr(inner),
                        _ => {
                            self.not_read("integer literal");
                            self.unknown(node, "integer literal")
                        }
                    },
                }
            }
            Kind::FloatingLiteral => Expr {
                kind: ExprKind::Float,
                ty,
            },
            Kind::StringLiteral | Kind::PredefinedExpr => match string_literal(node) {
                Some(bytes) => Expr {
                    kind: ExprKind::Str(Arc::from(bytes)),
                    ty,
                },
                None => self.unknown(node, "wide string literal"),
            },
            Kind::UnaryOperator => self.unary(node, ty),
            Kind::BinaryOperator => self.binary(node, ty),
            Kind::CompoundAssignOperator => {
                let [lhs, rhs] = node.inner.as_slice() else {
                    return self.unknown(node, "compound assignment shape");
                };
                let opcode = node.opcode.as_deref().unwrap_or("").trim_end_matches('=');
                let place = self.place(lhs);
                let rhs = self.expr(rhs);
                let Some(op) = BinOp::from_opcode(opcode) else {
                    self.not_read(format!("compound assignment {opcode}="));
                    return self.unknown(node, "compound assignment");
                };
                let current = Expr {
                    kind: ExprKind::Use(place.clone()),
                    ty: ty.clone(),
                };
                let value = if matches!(op, BinOp::Add | BinOp::Sub) && is_pointer_like(lhs) {
                    let pointer_ty = lhs.canonical_type().unwrap_or("").to_owned();
                    self.pointer_offset(current, rhs, op == BinOp::Sub, &pointer_ty, ty.clone())
                } else {
                    self.fold(Expr {
                        kind: ExprKind::Binary(op, Box::new(current), Box::new(rhs)),
                        ty: ty.clone(),
                    })
                };
                self.assign(place.clone(), value, line);
                Expr {
                    kind: ExprKind::Use(place),
                    ty,
                }
            }
            Kind::ConditionalOperator if node.inner.len() == 3 => {
                let on_true = self.new_block();
                let on_false = self.new_block();
                let join = self.new_block();
                let result = (&*ty != "void").then(|| self.temp(ty.clone()));
                self.cond(&node.inner[0], on_true, on_false);
                for (block, arm) in [(on_true, &node.inner[1]), (on_false, &node.inner[2])] {
                    self.current = block;
                    match result {
                        Some(id) => {
                            let value = self.expr(arm);
                            self.assign(Place::Var(id), value, arm.loc.line);
                        }
                        None => self.discard(arm),
                    }
                    self.set_term(Term::Goto(join));
                }
                self.current = join;
                match result {
                    Some(id) => Self::use_var(id, ty),
                    None => Expr {
                        kind: ExprKind::Int(0),
                        ty,
                    },
                }
            }
            Kind::BinaryConditionalOperator if node.inner.len() == 4 => {
                let common = self.expr(&node.inner[0]);
                let id = self.temp(ty.clone());
                let common_ty = common.ty.clone();
                self.assign(Place::Var(id), common, line);
                let on_false = self.new_block();
                let join = self.new_block();
                self.set_term(Term::Branch {
                    cond: Self::use_var(id, common_ty),
                    then: join,
                    els: on_false,
                });
                self.current = on_false;
                let value = self.expr(&node.inner[3]);
                self.assign(Place::Var(id), value, line);
                self.set_term(Term::Goto(join));
                self.current = join;
                Self::use_var(id, ty)
            }
            Kind::CallExpr => match self.call(node, true) {
                Some(value) => value,
                None => Expr {
                    kind: ExprKind::Int(0),
                    ty,
                },
            },
            Kind::MemberExpr | Kind::ArraySubscriptExpr | Kind::CompoundLiteralExpr => {
                let place = self.place(node);
                Expr {
                    kind: ExprKind::Use(place),
                    ty,
                }
            }
            Kind::UnaryExprOrTypeTraitExpr => {
                if let Some(value) = self.index.const_eval(node) {
                    return Expr {
                        kind: ExprKind::Int(value),
                        ty,
                    };
                }
                let of = match &node.arg_type {
                    Some(arg) => {
                        let arg = arg.to_string();
                        self.index.ty(&arg)
                    }
                    None => match node.inner.first() {
                        Some(inner) => self.ty(inner),
                        None => self.index.ty("void"),
                    },
                };
                let kind = match node.name.as_deref() {
                    Some("sizeof") => ExprKind::SizeOf(of),
                    Some("alignof" | "_Alignof" | "__alignof" | "__alignof__") => {
                        ExprKind::AlignOf(of)
                    }
                    other => ExprKind::Unknown(Arc::from(other.unwrap_or("type trait"))),
                };
                Expr { kind, ty }
            }
            Kind::StmtExpr => {
                let Some(body) = node.inner.first() else {
                    return self.unknown(node, "empty statement expression");
                };
                let count = body.inner.len();
                for (i, child) in body.inner.iter().enumerate() {
                    if i + 1 == count && &*ty != "void" && is_expression(&child.kind) {
                        return self.expr(child);
                    }
                    self.stmt(child);
                }
                Expr {
                    kind: ExprKind::Int(0),
                    ty,
                }
            }
            Kind::InitListExpr => {
                let id = self.temp(ty.clone());
                self.initialize(Place::Var(id), node);
                Self::use_var(id, ty)
            }
            Kind::ImplicitValueInitExpr => Expr {
                kind: ExprKind::Int(0),
                ty,
            },
            Kind::OpaqueValueExpr => match node.inner.first() {
                Some(source) => self.expr(source),
                None => self.unknown(node, "opaque value"),
            },
            Kind::AddrLabelExpr => {
                if let Some(id) = node.decl_id {
                    let block = self.label_block(id);
                    self.address_taken.push(block);
                }
                Expr {
                    kind: ExprKind::Unknown(Arc::from("label address")),
                    ty,
                }
            }
            Kind::AtomicExpr => {
                let args: Vec<Expr> = node.inner.iter().map(|arg| self.expr(arg)).collect();
                let name: FnName = Arc::from(node.name.as_deref().unwrap_or("__atomic"));
                let dst = (&*ty != "void").then(|| self.temp(ty.clone()));
                self.push(Stmt::Call {
                    dst: dst.map(Place::Var),
                    callee: Callee::Direct(name),
                    args,
                    line,
                });
                match dst {
                    Some(id) => Self::use_var(id, ty),
                    None => Expr {
                        kind: ExprKind::Int(0),
                        ty,
                    },
                }
            }
            Kind::GenericSelectionExpr => {
                let chosen = node
                    .inner
                    .iter()
                    .find(|association| association.is_selected)
                    .and_then(|association| {
                        association
                            .inner
                            .iter()
                            .rev()
                            .find(|c| is_expression(&c.kind))
                    });
                match chosen {
                    Some(chosen) => self.expr(chosen),
                    None => {
                        self.not_read("_Generic without a selected association");
                        self.unknown(node, "_Generic")
                    }
                }
            }
            Kind::ChooseExpr => {
                let [cond, yes, no] = node.inner.as_slice() else {
                    return self.unknown(node, "__builtin_choose_expr shape");
                };
                match self.index.const_eval(cond) {
                    Some(0) => self.expr(no),
                    Some(_) => self.expr(yes),
                    None => {
                        self.not_read("__builtin_choose_expr condition");
                        self.unknown(node, "__builtin_choose_expr")
                    }
                }
            }
            Kind::VaArgExpr => match node.inner.iter().find(|c| is_expression(&c.kind)) {
                Some(list) => {
                    let list = self.expr(list);
                    Expr {
                        kind: ExprKind::VaArg(Box::new(list)),
                        ty,
                    }
                }
                None => self.unknown(node, "va_arg without its list"),
            },
            // The catalog's `offsetof` is folded with the member access
            // beside it (`binary`); `__builtin_offsetof` written alone names
            // no member in the dump.
            Kind::OffsetOfExpr => {
                self.approximate("__builtin_offsetof without its members");
                self.unknown(node, "offsetof")
            }
            // Vectors' lanes are not followed: an operation on them gives
            // any value.
            Kind::ShuffleVectorExpr | Kind::ConvertVectorExpr => {
                for child in &node.inner {
                    if is_expression(&child.kind) {
                        self.discard(child);
                    }
                }
                self.unknown(node, "vector")
            }
            Kind::Other(name) => {
                let name = name.to_string();
                for child in &node.inner {
                    if is_expression(&child.kind) {
                        self.discard(child);
                    }
                }
                self.approximate(name.clone());
                self.unknown(node, &name)
            }
            other => {
                self.not_read(format!("{other:?} as an expression"));
                self.unknown(node, "unread expression")
            }
        }
    }

    fn unary(&mut self, node: &Node, ty: Ty) -> Expr {
        let Some(inner) = node.inner.first() else {
            return self.unknown(node, "unary operator without an operand");
        };
        let line = node.loc.line;
        match node.opcode.as_deref() {
            Some("&") => {
                if let Some(operand) = strip_parens(inner)
                    && operand.kind == Kind::DeclRefExpr
                    && operand
                        .referenced
                        .as_ref()
                        .is_some_and(|r| &*r.kind == "FunctionDecl")
                {
                    let mut value = self.expr(operand);
                    value.ty = ty;
                    return value;
                }
                let place = self.place(inner);
                Expr {
                    kind: ExprKind::AddrOf(place),
                    ty,
                }
            }
            Some("*") => {
                let place = self.place(node);
                Expr {
                    kind: ExprKind::Use(place),
                    ty,
                }
            }
            Some(op @ ("-" | "!" | "~")) => {
                let op = match op {
                    "-" => UnOp::Neg,
                    "!" => UnOp::Not,
                    _ => UnOp::BitNot,
                };
                let value = self.expr(inner);
                self.fold(Expr {
                    kind: ExprKind::Unary(op, Box::new(value)),
                    ty,
                })
            }
            Some("+" | "__extension__" | "__real" | "__imag") => self.expr(inner),
            Some(op @ ("++" | "--")) => {
                let place = self.place(inner);
                let current = Expr {
                    kind: ExprKind::Use(place.clone()),
                    ty: ty.clone(),
                };
                let old = node.is_postfix.then(|| {
                    let id = self.temp(ty.clone());
                    self.assign(Place::Var(id), current.clone(), line);
                    id
                });
                let one = self.int(1);
                let updated = if is_pointer_like(inner) {
                    let pointer_ty = inner.canonical_type().unwrap_or("").to_owned();
                    self.pointer_offset(current, one, op == "--", &pointer_ty, ty.clone())
                } else {
                    let bin = if op == "++" { BinOp::Add } else { BinOp::Sub };
                    Expr {
                        kind: ExprKind::Binary(bin, Box::new(current), Box::new(one)),
                        ty: ty.clone(),
                    }
                };
                self.assign(place.clone(), updated, line);
                match old {
                    Some(id) => Self::use_var(id, ty),
                    None => Expr {
                        kind: ExprKind::Use(place),
                        ty,
                    },
                }
            }
            other => {
                self.not_read(format!("unary operator {other:?}"));
                self.unknown(node, "unary operator")
            }
        }
    }

    fn binary(&mut self, node: &Node, ty: Ty) -> Expr {
        let [lhs, rhs] = node.inner.as_slice() else {
            return self.unknown(node, "binary operator shape");
        };
        let line = node.loc.line;
        if node.opcode.as_deref() == Some("+") {
            if let Some(offset) = self.index.offsetof_value(lhs, rhs) {
                return Expr {
                    kind: ExprKind::Int(i128::from(offset)),
                    ty,
                };
            }
            // A member at a variable index: its address in an object at
            // address zero.
            if let Some(member) = self.index.offsetof_member(lhs, rhs) {
                let address = self.expr(member);
                return self.fold(Expr {
                    kind: ExprKind::Cast(Box::new(address)),
                    ty,
                });
            }
        }
        match node.opcode.as_deref() {
            Some("=") => {
                let place = self.place(lhs);
                let value = self.expr(rhs);
                self.assign(place.clone(), value, line);
                Expr {
                    kind: ExprKind::Use(place),
                    ty,
                }
            }
            Some(",") => {
                self.discard(lhs);
                self.expr(rhs)
            }
            Some("&&" | "||") => {
                let id = self.temp(ty.clone());
                let yes = self.new_block();
                let no = self.new_block();
                let join = self.new_block();
                self.cond(node, yes, no);
                for (block, value) in [(yes, 1), (no, 0)] {
                    self.current = block;
                    let value = self.int(value);
                    self.assign(Place::Var(id), value, line);
                    self.set_term(Term::Goto(join));
                }
                self.current = join;
                Self::use_var(id, ty)
            }
            Some(opcode @ ("+" | "-")) if is_pointer_like(lhs) || is_pointer_like(rhs) => {
                let left = self.expr(lhs);
                let right = self.expr(rhs);
                let left_ty = lhs.canonical_type().unwrap_or("").to_owned();
                let right_ty = rhs.canonical_type().unwrap_or("").to_owned();
                match (is_pointer_like(lhs), is_pointer_like(rhs)) {
                    (true, true) => {
                        // The pointers' difference, in elements.
                        let size = self.index.pointee_size(&left_ty);
                        let difference = Expr {
                            kind: ExprKind::Binary(BinOp::Sub, Box::new(left), Box::new(right)),
                            ty: ty.clone(),
                        };
                        match size {
                            Some(size) if size > 1 => {
                                let size = Expr {
                                    kind: ExprKind::Int(i128::from(size)),
                                    ty: ty.clone(),
                                };
                                self.fold(Expr {
                                    kind: ExprKind::Binary(
                                        BinOp::Div,
                                        Box::new(difference),
                                        Box::new(size),
                                    ),
                                    ty,
                                })
                            }
                            _ => difference,
                        }
                    }
                    (true, false) => self.pointer_offset(left, right, opcode == "-", &left_ty, ty),
                    _ => self.pointer_offset(right, left, false, &right_ty, ty),
                }
            }
            Some(opcode) => match BinOp::from_opcode(opcode) {
                Some(op) => {
                    let left = self.expr(lhs);
                    let right = self.expr(rhs);
                    self.fold(Expr {
                        kind: ExprKind::Binary(op, Box::new(left), Box::new(right)),
                        ty,
                    })
                }
                None => {
                    self.not_read(format!("binary operator {opcode}"));
                    self.unknown(node, "binary operator")
                }
            },
            None => self.unknown(node, "binary operator without an opcode"),
        }
    }

    /// Lowers an expression naming an object to the place it names.
    fn place(&mut self, node: &Node) -> Place {
        match &node.kind {
            Kind::DeclRefExpr => {
                let Some(referenced) = node.referenced.as_ref() else {
                    self.not_read("reference without a declaration");
                    return self.unknown_place(node);
                };
                if let Some(&id) = self.locals.get(&referenced.id) {
                    return Place::Var(id);
                }
                if let Some(name) = self.block_globals.get(&referenced.id) {
                    return Place::Global(name.clone());
                }
                if let Some(name) = self.index.globals.get(&referenced.id) {
                    return Place::Global(name.clone());
                }
                match &*referenced.kind {
                    "VarDecl" => Place::Global(Arc::from(referenced.name.as_deref().unwrap_or(""))),
                    "FunctionDecl" => {
                        let value = self.expr(node);
                        Place::Deref(Box::new(value))
                    }
                    other => {
                        self.not_read(format!("reference to a {other}"));
                        self.unknown_place(node)
                    }
                }
            }
            Kind::ParenExpr | Kind::ExprWithCleanups => match node.inner.first() {
                Some(inner) => self.place(inner),
                None => self.unknown_place(node),
            },
            Kind::MemberExpr => {
                let Some(base) = node.inner.first() else {
                    return self.unknown_place(node);
                };
                let (record, name) = self.index.member_access(node, base);
                let field = self.index.field(record, name);
                let base_place = if node.is_arrow {
                    let pointer = self.expr(base);
                    Place::Deref(Box::new(pointer))
                } else {
                    self.place(base)
                };
                Place::Field(Box::new(base_place), field)
            }
            Kind::ArraySubscriptExpr => {
                let [first, second] = node.inner.as_slice() else {
                    return self.unknown_place(node);
                };
                let (base, index) = if is_pointer_like(first) || !is_pointer_like(second) {
                    (first, second)
                } else {
                    (second, first)
                };
                if base.kind == Kind::ImplicitCastExpr
                    && base.cast_kind.as_deref() == Some("ArrayToPointerDecay")
                    && let Some(array) = base.inner.first()
                    && string_literal(array).is_none()
                {
                    let array = self.place(array);
                    let index = self.expr(index);
                    let element = node.canonical_type().unwrap_or("").to_owned();
                    let size = self.index.size_of(&element);
                    let bytes = self.bytes(index, size);
                    return Place::Index(Box::new(array), Box::new(bytes));
                }
                if !is_pointer_like(base) {
                    // An element of a vector.
                    let vector = self.place(base);
                    let index = self.expr(index);
                    let element = node.canonical_type().unwrap_or("").to_owned();
                    let size = self.index.size_of(&element);
                    let bytes = self.bytes(index, size);
                    return Place::Index(Box::new(vector), Box::new(bytes));
                }
                let pointer_ty = base.canonical_type().unwrap_or("").to_owned();
                let pointer = self.expr(base);
                let index = self.expr(index);
                let ty = pointer.ty.clone();
                let element = self.pointer_offset(pointer, index, false, &pointer_ty, ty);
                Place::Deref(Box::new(element))
            }
            Kind::UnaryOperator if node.opcode.as_deref() == Some("*") => {
                match node.inner.first() {
                    Some(inner) => {
                        let pointer = self.expr(inner);
                        Place::Deref(Box::new(pointer))
                    }
                    None => self.unknown_place(node),
                }
            }
            Kind::UnaryOperator
                if matches!(
                    node.opcode.as_deref(),
                    Some("__extension__" | "__real" | "__imag")
                ) =>
            {
                match node.inner.first() {
                    Some(inner) => self.place(inner),
                    None => self.unknown_place(node),
                }
            }
            Kind::ImplicitCastExpr | Kind::CStyleCastExpr
                if matches!(node.cast_kind.as_deref(), Some("NoOp" | "LValueBitCast"))
                    && !node.inner.is_empty() =>
            {
                self.place(&node.inner[0])
            }
            Kind::CompoundLiteralExpr => {
                let ty = self.ty(node);
                let id = self.temp(ty);
                if let Some(init) = node.inner.iter().find(|c| is_expression(&c.kind)) {
                    self.initialize(Place::Var(id), init);
                }
                Place::Var(id)
            }
            Kind::StringLiteral | Kind::PredefinedExpr => {
                let value = self.expr(node);
                Place::Deref(Box::new(value))
            }
            _ => {
                let value = self.expr(node);
                let id = self.temp(value.ty.clone());
                self.assign(Place::Var(id), value, node.loc.line);
                Place::Var(id)
            }
        }
    }

    fn unknown_place(&mut self, node: &Node) -> Place {
        let unknown = self.unknown(node, "unread place");
        Place::Deref(Box::new(unknown))
    }

    // Global initializers.

    fn init(&mut self, node: &Node) -> Init {
        self.init_record(node, None)
    }

    /// A global's initializer, whose record, or its elements', is `record`
    /// where the member holding it tells.
    fn init_record(&mut self, node: &Node, record: Option<Arc<str>>) -> Init {
        match node.kind {
            Kind::InitListExpr => {
                let ty = node.canonical_type().unwrap_or("").to_owned();
                if ty.trim_end().ends_with(']') {
                    let element = self.index.size_of(&array_element(&ty));
                    let mut items = Vec::new();
                    for (i, item) in node.inner.iter().enumerate() {
                        items.push(InitItem {
                            field: None,
                            offset: element.map(|size| size * i as u64),
                            init: self.init_record(item, record.clone()),
                        });
                    }
                    let filler = node
                        .array_filler
                        .as_deref()
                        .map(|filler| Box::new(self.init_record(filler, record.clone())));
                    return Init::List {
                        items,
                        filler,
                        element,
                    };
                }
                let record = record.unwrap_or_else(|| self.index.record_of(&ty));
                let members = self.index.record_members(&record);
                let mut items = Vec::new();
                if let Some(field) = &node.field {
                    let (record, name) = self.index.member(field.id).unwrap_or_else(|| {
                        (record, Arc::from(field.name.as_deref().unwrap_or("")))
                    });
                    let held = members
                        .iter()
                        .find(|m| m.name == name)
                        .and_then(|m| m.record.clone());
                    let field = self.index.field(record, name);
                    let init = match node.inner.first() {
                        Some(value) => self.init_record(value, held),
                        None => Init::Zero,
                    };
                    items.push(InitItem {
                        offset: field.offset,
                        field: Some(field),
                        init,
                    });
                } else {
                    for (i, item) in node.inner.iter().enumerate() {
                        let member = members.get(i);
                        let name =
                            member.map_or_else(|| Arc::from(format!("#{i}")), |m| m.name.clone());
                        let held = member.and_then(|m| m.record.clone());
                        let field = self.index.field(record.clone(), name);
                        let init = self.init_record(item, held);
                        items.push(InitItem {
                            offset: field.offset,
                            field: Some(field),
                            init,
                        });
                    }
                }
                Init::List {
                    items,
                    filler: None,
                    element: None,
                }
            }
            Kind::ImplicitValueInitExpr => Init::Zero,
            Kind::ParenExpr | Kind::ConstantExpr if node.inner.len() == 1 => {
                match self.index.const_eval(node) {
                    Some(value) => Init::Expr(Expr {
                        kind: ExprKind::Int(value),
                        ty: self.ty(node),
                    }),
                    None => self.init_record(&node.inner[0], record),
                }
            }
            _ => {
                let value = self.expr(node);
                Init::Expr(value)
            }
        }
    }
}

/// `__builtin_types_compatible_p`, the type trait C has: whether its two
/// types are the same once their outermost qualifiers go.
fn types_compatible(index: &mut TuIndex, node: &Node) -> Option<bool> {
    let types: Vec<&Node> = node
        .inner
        .iter()
        .filter(|child| matches!(&child.kind, Kind::Other(name) if name.ends_with("Type")))
        .collect();
    let [first, second] = types.as_slice() else {
        return None;
    };
    let first = type_node_text(first)?.to_owned();
    let second = type_node_text(second)?.to_owned();
    let first = index.ty(&first);
    let second = index.ty(&second);
    Some(strip_qualifiers(&first) == strip_qualifiers(&second))
}

/// The type a type node stands for, through `typeof` and parentheses.
fn type_node_text(node: &Node) -> Option<&str> {
    if let Some(desugared) = node.desugared.as_deref() {
        return Some(desugared);
    }
    match &node.kind {
        Kind::Other(name) if matches!(&**name, "TypeOfExprType" | "TypeOfType" | "ParenType") => {
            let under = node
                .inner
                .iter()
                .rev()
                .find(|c| matches!(&c.kind, Kind::Other(n) if n.ends_with("Type")))?;
            type_node_text(under)
        }
        _ => node.ty.as_deref(),
    }
}

/// A callee expression's declaration reference, through its casts and
/// parentheses.
fn strip_to_decl_ref(node: &Node) -> Option<&Node> {
    match node.kind {
        Kind::DeclRefExpr => Some(node),
        Kind::ImplicitCastExpr | Kind::ParenExpr => strip_to_decl_ref(node.inner.first()?),
        _ => None,
    }
}

fn strip_parens(node: &Node) -> Option<&Node> {
    match node.kind {
        Kind::ParenExpr => strip_parens(node.inner.first()?),
        _ => Some(node),
    }
}

/// Whether an expression is a pointer or an array, the base of a
/// subscript.
fn is_pointer_like(node: &Node) -> bool {
    node.canonical_type()
        .is_some_and(|ty| ty.trim_end().ends_with('*') || ty.trim_end().ends_with(']'))
}

/// The bytes of a string literal, through parentheses and `__func__`.
fn string_literal(node: &Node) -> Option<Vec<u8>> {
    match (&node.kind, &node.value) {
        (Kind::StringLiteral, Some(Literal::Text(spelled))) => unescape(spelled),
        (Kind::ParenExpr | Kind::PredefinedExpr, _) => string_literal(node.inner.first()?),
        _ => None,
    }
}
