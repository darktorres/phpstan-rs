//! What the build registers with the engine: each builtin function and
//! method with the C functions implementing it, each class with the globals
//! holding its entry.

use std::collections::BTreeMap;
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::ir::{
    BinOp, BlockId, Callee, Expr, ExprKind, FnName, Function, GlobalName, Init, InitItem, Place,
    Stmt, Term, UnOp, VarId,
};
use crate::program::Program;
use crate::store::Store;

/// A function entry (`ZEND_FE`, `ZEND_ME` and their kin).
#[derive(Debug, Clone)]
pub(crate) struct Entry {
    /// The handler; none for an abstract method.
    pub(crate) handler: Option<FnName>,
    /// The frameless variants the compiler may call instead, by their
    /// number of arguments.
    pub(crate) frameless: Vec<FnName>,
    pub(crate) flags: i128,
    /// The array holding the entry.
    pub(crate) array: GlobalName,
}

#[derive(Debug, Default)]
pub(crate) struct Class {
    /// The globals the class's entry is stored in.
    pub(crate) globals: Vec<GlobalName>,
    /// Each method's entries, by its name as declared.
    pub(crate) methods: BTreeMap<String, Vec<Entry>>,
    /// The method arrays registered with it.
    pub(crate) arrays: Vec<GlobalName>,
    /// The methods a PDO driver gives the class's objects it made, by the
    /// driver's name, then by the method's name as declared: found by the
    /// objects' `get_method` handler, not in the class.
    pub(crate) driver_methods: BTreeMap<String, BTreeMap<String, Vec<Entry>>>,
}

#[derive(Debug, Default)]
pub(crate) struct Registry {
    /// Each builtin function's entries, by its name as declared: one for
    /// each module or SAPI declaring it.
    pub(crate) functions: BTreeMap<String, Vec<Entry>>,
    pub(crate) classes: BTreeMap<String, Class>,
    /// The function arrays a module or a SAPI registers.
    pub(crate) function_arrays: Vec<GlobalName>,
    /// The entry arrays with entries that nothing found registers.
    pub(crate) unregistered: Vec<GlobalName>,
}

/// A class a value is the entry of, by its name.
type ClassName = Arc<str>;

pub(crate) fn find(program: &Program, store: &Store) -> Result<Registry, String> {
    let arrays = entry_arrays(program);
    let mut registered_functions = function_array_references(program);
    // One pass over the functions' code: the function arrays they pass
    // on, and the ones that may register a class or give one's entry.
    let mut registering = Vec::new();
    program.each_function(store, |function| {
        function_array_assignments(function, &mut registered_functions);
        if may_register_class(function) {
            registering.push(function.clone());
        }
    })?;
    let classes = class_registrations(&registering);
    let mut registry = Registry::default();
    let mut method_arrays: FxHashMap<&GlobalName, &str> = FxHashMap::default();
    for (name, registration) in &classes.classes {
        let class = registry.classes.entry(name.to_string()).or_default();
        for array in &registration.arrays {
            if !class.arrays.contains(array) {
                class.arrays.push(array.clone());
            }
            method_arrays.insert(array, name);
        }
    }
    for (global, name) in &classes.globals {
        let class = registry.classes.entry(name.to_string()).or_default();
        if !class.globals.contains(global) {
            class.globals.push(global.clone());
        }
    }
    let mut driver_arrays: FxHashSet<&GlobalName> = FxHashSet::default();
    let driver_methods = pdo_driver_methods(program, store)?;
    for (class, driver, array) in &driver_methods {
        let Some(entries) = arrays.get(array) else {
            continue;
        };
        driver_arrays.insert(array);
        let methods = registry
            .classes
            .entry((*class).to_owned())
            .or_default()
            .driver_methods
            .entry(driver.clone())
            .or_default();
        for (name, entry) in entries {
            let entries = methods.entry(name.clone()).or_default();
            if !entries.iter().any(|e| e.array == entry.array) {
                entries.push(entry.clone());
            }
        }
    }
    let mut names: Vec<&GlobalName> = arrays.keys().collect();
    names.sort_unstable();
    for array in names {
        let entries = &arrays[array];
        if driver_arrays.contains(array) {
            continue;
        }
        if let Some(class) = method_arrays.get(array) {
            let class = registry
                .classes
                .get_mut(*class)
                .expect("each class registering an array is listed");
            for (name, entry) in entries {
                class
                    .methods
                    .entry(name.clone())
                    .or_default()
                    .push(entry.clone());
            }
        } else if registered_functions.contains_key(array) {
            registry.function_arrays.push(array.clone());
            for (name, entry) in entries {
                registry
                    .functions
                    .entry(name.clone())
                    .or_default()
                    .push(entry.clone());
            }
        } else if !entries.is_empty() {
            registry.unregistered.push(array.clone());
        }
    }
    Ok(registry)
}

/// Each array of function entries, its entries by name.
fn entry_arrays(program: &Program) -> FxHashMap<GlobalName, Vec<(String, Entry)>> {
    let mut arrays = FxHashMap::default();
    for (name, definitions) in &program.globals {
        for global in definitions {
            if !global.ty.contains("zend_function_entry") || !global.ty.ends_with(']') {
                continue;
            }
            let Some(Init::List { items, .. }) = &global.init else {
                continue;
            };
            let mut entries = Vec::new();
            for item in items {
                let Init::List { items: fields, .. } = &item.init else {
                    continue;
                };
                let Some(entry_name) = member_init(fields, "fname").and_then(init_string) else {
                    continue;
                };
                let entry = Entry {
                    handler: member_init(fields, "handler").and_then(init_function),
                    frameless: member_init(fields, "frameless_function_infos")
                        .and_then(init_global)
                        .map(|infos| frameless_variants(program, &infos))
                        .unwrap_or_default(),
                    flags: member_init(fields, "flags").and_then(init_int).unwrap_or(0),
                    array: name.clone(),
                };
                entries.push((String::from_utf8_lossy(&entry_name).into_owned(), entry));
            }
            arrays.insert(name.clone(), entries);
        }
    }
    arrays
}

/// The functions an array of frameless function infos lists.
fn frameless_variants(program: &Program, infos: &GlobalName) -> Vec<FnName> {
    let mut variants = Vec::new();
    for global in program.globals.get(infos).into_iter().flatten() {
        let Some(Init::List { items, .. }) = &global.init else {
            continue;
        };
        for item in items {
            if let Init::List { items: fields, .. } = &item.init
                && let Some(handler) = member_init(fields, "handler").and_then(init_function)
                && !variants.contains(&handler)
            {
                variants.push(handler);
            }
        }
    }
    variants
}

/// The arrays a module entry, a SAPI or a direct registration names as
/// functions, each with what names it.
fn function_array_references(program: &Program) -> FxHashMap<GlobalName, Vec<Arc<str>>> {
    let mut references: FxHashMap<GlobalName, Vec<Arc<str>>> = FxHashMap::default();
    for (name, definitions) in &program.globals {
        for global in definitions {
            let Some(Init::List { items, .. }) = &global.init else {
                continue;
            };
            for item in items {
                if let Some(field) = &item.field
                    && MODULE_FIELDS
                        .iter()
                        .any(|(r, f)| *field.record == **r && *field.name == **f)
                    && let Some(array) = init_global(&item.init)
                {
                    references.entry(array).or_default().push(name.clone());
                }
            }
        }
    }
    references
}

/// The members of the module and SAPI records holding a function array.
const MODULE_FIELDS: [(&str, &str); 2] = [
    ("_zend_module_entry", "functions"),
    ("_sapi_module_struct", "additional_functions"),
];

/// A function's assignments of function arrays to the members that hold
/// one, and the arrays it passes to the engine's own registration.
fn function_array_assignments(
    function: &Function,
    references: &mut FxHashMap<GlobalName, Vec<Arc<str>>>,
) {
    for block in &function.blocks {
        for stmt in &block.stmts {
            match stmt {
                Stmt::Assign {
                    dst: Place::Field(_, field),
                    src,
                    ..
                } if MODULE_FIELDS
                    .iter()
                    .any(|(r, f)| *field.record == **r && *field.name == **f) =>
                {
                    if let Some(array) = expr_global(src) {
                        references
                            .entry(array)
                            .or_default()
                            .push(function.name.clone());
                    }
                }
                Stmt::Call {
                    callee: Callee::Direct(callee),
                    args,
                    ..
                } if &**callee == "zend_register_functions" => {
                    if let Some(array) = args.get(1).and_then(expr_global) {
                        references
                            .entry(array)
                            .or_default()
                            .push(function.name.clone());
                    }
                }
                _ => {}
            }
        }
    }
}

/// Whether a function may register a class or give a class's entry: it
/// calls a registration, or a function whose result it keeps as a class
/// entry.
fn may_register_class(function: &Function) -> bool {
    function.blocks.iter().flat_map(|b| &b.stmts).any(|stmt| {
        let Stmt::Call {
            dst, callee: Callee::Direct(callee), ..
        } = stmt
        else {
            return false;
        };
        REGISTER_FROM_ENTRY.contains(&&**callee)
            || &**callee == "zend_register_internal_enum"
            || matches!(dst, Some(Place::Var(var))
                if function.vars.get(*var as usize).is_some_and(|v| v.ty.contains("zend_class_entry")))
    })
}

/// The classes whose objects take the methods a PDO driver's
/// `get_driver_methods` hook gives, by the kind of method it is asked for
/// (`PDO_DBH_DRIVER_METHOD_KIND_DBH`, `PDO_DBH_DRIVER_METHOD_KIND_STMT`).
const PDO_DRIVER_METHOD_CLASSES: [&str; 2] = ["PDO", "PDOStatement"];

/// The method arrays each PDO driver gives the objects of a class, as the
/// class, the driver's name and the array: the driver's handle factory
/// sets the handle's methods, whose `get_driver_methods` hook returns an
/// array for each kind of method.
fn pdo_driver_methods(
    program: &Program,
    store: &Store,
) -> Result<Vec<(&'static str, String, GlobalName)>, String> {
    let mut found = Vec::new();
    let mut drivers: Vec<(&GlobalName, &Vec<_>)> = program.globals.iter().collect();
    drivers.sort_unstable_by_key(|(name, _)| *name);
    for global in drivers.into_iter().flat_map(|(_, definitions)| definitions) {
        let Some(Init::List { items, .. }) = &global.init else {
            continue;
        };
        let (Some(name), Some(factory)) = (
            record_member_init(items, "pdo_driver_t", "driver_name").and_then(init_string),
            record_member_init(items, "pdo_driver_t", "db_handle_factory").and_then(init_function),
        ) else {
            continue;
        };
        let driver_name = String::from_utf8_lossy(&name).into_owned();
        for factory in program.load(store, &factory)? {
            for table in assigned_globals(&factory, "_pdo_dbh_t", "methods") {
                for table in program.globals.get(&table).into_iter().flatten() {
                    let Some(Init::List { items, .. }) = &table.init else {
                        continue;
                    };
                    let Some(hook) =
                        record_member_init(items, "pdo_dbh_methods", "get_driver_methods")
                            .and_then(init_function)
                    else {
                        continue;
                    };
                    for hook in program.load(store, &hook)? {
                        for (kinds, array) in
                            returned_arrays(&hook, PDO_DRIVER_METHOD_CLASSES.len())
                        {
                            for (kind, class) in PDO_DRIVER_METHOD_CLASSES.iter().enumerate() {
                                let entry = (*class, driver_name.clone(), array.clone());
                                if kinds & (1 << kind) != 0 && !found.contains(&entry) {
                                    found.push(entry);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(found)
}

/// The globals whose address a function stores in a record's member.
fn assigned_globals(function: &Function, record: &str, member: &str) -> Vec<GlobalName> {
    let mut globals = Vec::new();
    for block in &function.blocks {
        for stmt in &block.stmts {
            if let Stmt::Assign {
                dst: Place::Field(_, field),
                src,
                ..
            } = stmt
                && *field.record == *record
                && *field.name == *member
                && let Some(global) = expr_global(src)
                && !globals.contains(&global)
            {
                globals.push(global);
            }
        }
    }
    globals
}

/// The arrays a function returns, each with the values of its second
/// parameter, below `kinds`, it may return the array for (a bit for
/// each): narrowed by the switches and the comparisons with a constant
/// on the parameter along the way, unless the function assigns it.
fn returned_arrays(function: &Function, kinds: usize) -> Vec<(u32, GlobalName)> {
    const PARAMETER: VarId = 1;
    let all: u32 = (1 << kinds) - 1;
    let assigned = function.blocks.iter().flat_map(|b| &b.stmts).any(|stmt| {
        matches!(
            stmt,
            Stmt::Assign {
                dst: Place::Var(PARAMETER),
                ..
            } | Stmt::Call {
                dst: Some(Place::Var(PARAMETER)),
                ..
            }
        )
    });
    let narrows = function.params > PARAMETER && !assigned;
    let is_parameter = |expr: &Expr| narrows && expr_var(expr) == Some(PARAMETER);
    let range = |low: i128, high: i128| {
        (0..kinds)
            .filter(|&k| (low..=high).contains(&(k as i128)))
            .fold(0, |mask, k| mask | (1 << k))
    };
    // The values a condition holds for, where it tests the parameter.
    let test = |cond: &Expr| -> Option<u32> {
        let cond = strip_cast(cond);
        let (negated, cond) = match &cond.kind {
            ExprKind::Unary(UnOp::Not, inner) => (true, strip_cast(inner)),
            _ => (false, cond),
        };
        let mask = match &cond.kind {
            ExprKind::Binary(op @ (BinOp::Eq | BinOp::Ne), a, b) => {
                let value = match (&strip_cast(a).kind, &strip_cast(b).kind) {
                    (_, ExprKind::Int(value)) if is_parameter(a) => *value,
                    (ExprKind::Int(value), _) if is_parameter(b) => *value,
                    _ => return None,
                };
                let equal = range(value, value);
                if *op == BinOp::Eq {
                    equal
                } else {
                    all & !equal
                }
            }
            _ if is_parameter(cond) => all & !range(0, 0),
            _ => return None,
        };
        Some(if negated { all & !mask } else { mask })
    };
    let mut reached = vec![0u32; function.blocks.len()];
    let mut returned: Vec<(u32, GlobalName)> = Vec::new();
    let mut work: Vec<(BlockId, u32)> = vec![(0, all)];
    while let Some((id, kinds)) = work.pop() {
        let Some(seen) = reached.get_mut(id as usize) else {
            continue;
        };
        if kinds == 0 || *seen | kinds == *seen {
            continue;
        }
        *seen |= kinds;
        match &function.blocks[id as usize].term {
            Term::Goto(next) => work.push((*next, kinds)),
            Term::IndirectGoto(targets) => {
                work.extend(targets.iter().map(|target| (*target, kinds)));
            }
            Term::Branch { cond, then, els } => {
                let holds = test(cond).unwrap_or(all);
                let fails = test(cond).map_or(all, |mask| all & !mask);
                work.push((*then, kinds & holds));
                work.push((*els, kinds & fails));
            }
            Term::Switch { on, cases, default } => {
                let mut rest = kinds;
                for (low, high, target) in cases {
                    if is_parameter(on) {
                        let mask = range(*low, *high);
                        work.push((*target, kinds & mask));
                        rest &= !mask;
                    } else {
                        work.push((*target, kinds));
                    }
                }
                work.push((*default, rest));
            }
            Term::Return(Some(value)) => {
                if let Some(array) = expr_global(value) {
                    match returned.iter_mut().find(|(_, a)| *a == array) {
                        Some((mask, _)) => *mask |= kinds,
                        None => returned.push((kinds, array)),
                    }
                }
            }
            Term::Return(None) | Term::Unreachable => {}
        }
    }
    returned.sort_unstable_by(|a, b| a.1.cmp(&b.1));
    returned
}

/// What the class registrations give: each class's method arrays, and the
/// globals holding each class's entry.
#[derive(Default)]
struct ClassRegistrations {
    classes: BTreeMap<ClassName, ClassRegistration>,
    globals: BTreeMap<GlobalName, ClassName>,
}

#[derive(Default)]
struct ClassRegistration {
    arrays: Vec<GlobalName>,
}

/// The engine's functions that register a class from an entry made by
/// `INIT_CLASS_ENTRY`: the entry, then what they give.
const REGISTER_FROM_ENTRY: &[&str] = &[
    "zend_register_internal_class",
    "zend_register_internal_class_ex",
    "zend_register_internal_class_with_flags",
    "zend_register_internal_interface",
];

/// Reads the class registrations: a function that fills a class entry's
/// name and methods and registers it gives the registered class, and a
/// global that a registered class's entry is stored in holds it, through
/// the functions returning one.
fn class_registrations(functions: &[Function]) -> ClassRegistrations {
    let mut found = ClassRegistrations::default();
    let mut returns: FxHashMap<FnName, ClassName> = FxHashMap::default();
    loop {
        let before = (returns.len(), found.globals.len());
        for function in functions {
            if let Some(class) = registrations_in(function, &returns, &mut found) {
                returns.insert(function.name.clone(), class);
            }
        }
        if (returns.len(), found.globals.len()) == before {
            return found;
        }
    }
}

/// Reads one function's registrations; gives the class it returns the
/// entry of, if it does.
fn registrations_in(
    function: &Function,
    returns: &FxHashMap<FnName, ClassName>,
    found: &mut ClassRegistrations,
) -> Option<ClassName> {
    // The string each variable is made from, the class entry each holds,
    // and the name and methods each local entry is given.
    let mut strings: FxHashMap<VarId, Arc<[u8]>> = FxHashMap::default();
    let mut classes: FxHashMap<VarId, ClassName> = FxHashMap::default();
    let mut entry_names: FxHashMap<VarId, ClassName> = FxHashMap::default();
    let mut entry_arrays: FxHashMap<VarId, Vec<GlobalName>> = FxHashMap::default();
    let mut returned = None;
    // Twice, so that a value made after its use in the order of blocks
    // still reaches it.
    for _ in 0..2 {
        for block in &function.blocks {
            for stmt in &block.stmts {
                match stmt {
                    Stmt::Assign { dst, src, .. } => match dst {
                        Place::Var(var) => {
                            if let Some(string) = expr_string(src, &strings) {
                                strings.insert(*var, string);
                            }
                            if let Some(class) = expr_var(src).and_then(|v| classes.get(&v)) {
                                classes.insert(*var, class.clone());
                            }
                        }
                        Place::Global(global) => {
                            if let Some(class) = expr_var(src).and_then(|v| classes.get(&v)) {
                                found.globals.insert(global.clone(), class.clone());
                            }
                        }
                        Place::Field(base, field)
                            if &*field.record == "_zend_class_entry" && &*field.name == "name" =>
                        {
                            if let (Place::Var(entry), Some(string)) =
                                (&**base, expr_string(src, &strings))
                            {
                                entry_names.insert(*entry, class_name(&string));
                            }
                        }
                        Place::Field(_, field) if &*field.name == "builtin_functions" => {
                            if let (Some(entry), Some(array)) =
                                (entry_var_of(dst), expr_global(src))
                            {
                                let arrays = entry_arrays.entry(entry).or_default();
                                if !arrays.contains(&array) {
                                    arrays.push(array);
                                }
                            }
                        }
                        _ => {}
                    },
                    Stmt::Call {
                        dst, callee, args, ..
                    } => {
                        let class = match callee {
                            Callee::Direct(callee) if REGISTER_FROM_ENTRY.contains(&&**callee) => {
                                let entry = args.first().and_then(expr_addressed_var);
                                entry.and_then(|entry| {
                                    let name = entry_names.get(&entry)?.clone();
                                    let registration =
                                        found.classes.entry(name.clone()).or_default();
                                    for array in entry_arrays.get(&entry).into_iter().flatten() {
                                        if !registration.arrays.contains(array) {
                                            registration.arrays.push(array.clone());
                                        }
                                    }
                                    Some(name)
                                })
                            }
                            Callee::Direct(callee)
                                if &**callee == "zend_register_internal_enum" =>
                            {
                                let name = args.first().and_then(|a| expr_string(a, &strings));
                                name.map(|name| {
                                    let name = class_name(&name);
                                    let registration =
                                        found.classes.entry(name.clone()).or_default();
                                    if let Some(array) = args.get(2).and_then(expr_global)
                                        && !registration.arrays.contains(&array)
                                    {
                                        registration.arrays.push(array);
                                    }
                                    name
                                })
                            }
                            Callee::Direct(callee) => returns.get(callee).cloned(),
                            Callee::Indirect(_) => None,
                        };
                        if let Some(Place::Var(var)) = dst {
                            if let Some(class) = class {
                                classes.insert(*var, class);
                            } else if let Some(string) =
                                args.first().and_then(|a| expr_string(a, &strings))
                            {
                                strings.insert(*var, string);
                            }
                        }
                    }
                    Stmt::Asm { .. } => {}
                }
            }
            if let Term::Return(Some(value)) = &block.term
                && let Some(class) = expr_var(value).and_then(|v| classes.get(&v))
            {
                returned = Some(class.clone());
            }
        }
    }
    returned
}

fn class_name(bytes: &[u8]) -> ClassName {
    Arc::from(String::from_utf8_lossy(bytes).as_ref())
}

/// The local entry a member of it is written in (`ce.info.internal.x`).
fn entry_var_of(place: &Place) -> Option<VarId> {
    match place {
        Place::Var(var) => Some(*var),
        Place::Field(base, _) => entry_var_of(base),
        _ => None,
    }
}

fn strip_cast(expr: &Expr) -> &Expr {
    match &expr.kind {
        ExprKind::Cast(inner) => strip_cast(inner),
        _ => expr,
    }
}

fn expr_var(expr: &Expr) -> Option<VarId> {
    match &strip_cast(expr).kind {
        ExprKind::Use(Place::Var(var)) => Some(*var),
        _ => None,
    }
}

/// The local variable whose address an expression is.
fn expr_addressed_var(expr: &Expr) -> Option<VarId> {
    match &strip_cast(expr).kind {
        ExprKind::AddrOf(Place::Var(var)) => Some(*var),
        _ => None,
    }
}

fn expr_string(expr: &Expr, strings: &FxHashMap<VarId, Arc<[u8]>>) -> Option<Arc<[u8]>> {
    match &strip_cast(expr).kind {
        ExprKind::Str(bytes) => Some(bytes.clone()),
        ExprKind::Use(Place::Var(var)) => strings.get(var).cloned(),
        _ => None,
    }
}

/// The global an expression gives the address of, or whose array decays.
fn expr_global(expr: &Expr) -> Option<GlobalName> {
    match &strip_cast(expr).kind {
        ExprKind::AddrOf(Place::Global(global)) => Some(global.clone()),
        ExprKind::AddrOf(Place::Index(base, _)) => match &**base {
            Place::Global(global) => Some(global.clone()),
            _ => None,
        },
        ExprKind::Use(Place::Global(global)) => Some(global.clone()),
        _ => None,
    }
}

/// What an initializer list gives a member, by the member's name.
fn member_init<'i>(items: &'i [InitItem], name: &str) -> Option<&'i Init> {
    items
        .iter()
        .find(|item| item.field.as_ref().is_some_and(|f| &*f.name == name))
        .map(|item| &item.init)
}

/// What an initializer list gives a member of a record, by their names.
fn record_member_init<'i>(items: &'i [InitItem], record: &str, name: &str) -> Option<&'i Init> {
    items
        .iter()
        .find(|item| {
            item.field
                .as_ref()
                .is_some_and(|f| &*f.record == record && &*f.name == name)
        })
        .map(|item| &item.init)
}

fn init_expr(init: &Init) -> Option<&Expr> {
    match init {
        Init::Expr(expr) => Some(expr),
        _ => None,
    }
}

fn init_string(init: &Init) -> Option<Arc<[u8]>> {
    expr_string(init_expr(init)?, &FxHashMap::default())
}

fn init_function(init: &Init) -> Option<FnName> {
    match &strip_cast(init_expr(init)?).kind {
        ExprKind::Func(name) => Some(name.clone()),
        _ => None,
    }
}

fn init_global(init: &Init) -> Option<GlobalName> {
    expr_global(init_expr(init)?)
}

fn init_int(init: &Init) -> Option<i128> {
    match &strip_cast(init_expr(init)?).kind {
        ExprKind::Int(value) => Some(*value),
        _ => None,
    }
}

impl Registry {
    /// The registrations as lines of tab-separated fields: `function`, its
    /// name, handler, frameless variants and array; `method`, its class,
    /// name, flags, handler and array; `driver-method`, its class, driver,
    /// name, flags, handler and array; `class`, its name and globals;
    /// `unregistered` and an array.
    pub(crate) fn to_tsv(&self) -> String {
        let mut out = String::new();
        let handler = |entry: &Entry| entry.handler.as_deref().unwrap_or("-").to_owned();
        for (name, entries) in &self.functions {
            for entry in entries {
                let frameless: Vec<&str> = entry.frameless.iter().map(|f| &**f).collect();
                out.push_str(&format!(
                    "function\t{name}\t{}\t{}\t{}\n",
                    handler(entry),
                    frameless.join(","),
                    entry.array
                ));
            }
        }
        for (class_name, class) in &self.classes {
            let globals: Vec<&str> = class.globals.iter().map(|g| &**g).collect();
            out.push_str(&format!("class\t{class_name}\t{}\n", globals.join(",")));
            for (name, entries) in &class.methods {
                for entry in entries {
                    out.push_str(&format!(
                        "method\t{class_name}\t{name}\t{:#x}\t{}\t{}\n",
                        entry.flags,
                        handler(entry),
                        entry.array
                    ));
                }
            }
            for (driver, methods) in &class.driver_methods {
                for (name, entries) in methods {
                    for entry in entries {
                        out.push_str(&format!(
                            "driver-method\t{class_name}\t{driver}\t{name}\t{:#x}\t{}\t{}\n",
                            entry.flags,
                            handler(entry),
                            entry.array
                        ));
                    }
                }
            }
        }
        for array in &self.unregistered {
            out.push_str(&format!("unregistered\t{array}\n"));
        }
        out
    }
}
