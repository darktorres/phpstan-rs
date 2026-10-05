//! The libraries the build has no source of, taken by their types, as C's
//! conversions allow.
//!
//! - A pointer is classed by what it points to and whether that is
//!   `const`: a record the library sees, bytes, a pointer of a class, a
//!   function, other data, or `void`. A record of php-src's own is `void`
//!   to a library; an integer holds no pointer across its bounds.
//! - A library holds each pointer it is given by its class, and how much
//!   of what it points to it reaches: one element, every element of an
//!   array, or a number of bytes.
//! - It gives at a class what it holds of the class, of the class's
//!   pointers that are not `const` where the class is, and its own memory
//!   of the class; at `void` every pointer it holds and all its own
//!   memory; at a function the functions it holds and its own code.
//! - It reads and writes through each pointer it holds at the pointer's
//!   type: a record's members by the layout, a pointer's target. What it
//!   reads it holds; what it writes it gives, at the member's class, and
//!   the outside value into bytes. A `const` pointer it only reads, and a
//!   `void` one it never reads or writes.
//! - It calls each function it holds from every call into it, each
//!   parameter given what it gives at the parameter's class, and holds
//!   the result.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::{
    COPY, Cons, LocId, NONE, NodeId, ObjKind, Off, Pta, Site, SiteId, SiteKind, Transform, TyId,
};
use crate::libraries::LibId;
use crate::types::Shape;

pub(crate) type ClassId = u32;

/// What a pointer points to, as a library's types tell.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Pointee {
    /// Memory the library never reads or writes.
    Void,
    /// Characters, or `void` with its length beside it.
    Bytes,
    /// Data holding no pointer.
    Data,
    /// A record the library sees, by the catalog's name for it.
    Record(Arc<str>),
    /// A pointer of a class.
    Pointer(ClassId),
    Function,
}

/// A pointer's class.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Class {
    pub(crate) to: Pointee,
    pub(crate) konst: bool,
}

/// How much of what a pointer points to a library reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Extent {
    /// The element at the pointer; for bytes, every byte up to the
    /// object's end.
    One,
    /// Every element of an array of them.
    Array,
    /// As many bytes.
    Len(u64),
}

/// A library's places.
pub(crate) struct LibState {
    /// Its code.
    pub(crate) code: LocId,
    /// Its calls of the functions it holds.
    pub(crate) site: SiteId,
    /// What it holds, by class and by how much of what each points to it
    /// reaches.
    pub(crate) held: FxHashMap<(ClassId, Extent), NodeId>,
    /// What it gives, by class.
    pub(crate) gives: FxHashMap<ClassId, NodeId>,
    /// Its own memory, by the class of a pointer to it.
    pub(crate) own: FxHashMap<ClassId, LocId>,
}

/// A member of a record as a library reaches it: a pointer of a type, or
/// bytes.
#[derive(Debug, Clone)]
pub(crate) enum Member {
    Pointer(i64, Arc<str>),
    Bytes(i64, u32),
}

/// The classes and what is remembered of types for them.
#[derive(Default)]
pub(crate) struct Classes {
    classes: Vec<Class>,
    ids: FxHashMap<Class, ClassId>,
    /// Each type's class, by whether every record is seen.
    of_type: FxHashMap<(bool, Arc<str>), Option<ClassId>>,
    members: FxHashMap<Arc<str>, Arc<[Member]>>,
}

impl Pta<'_> {
    /// A type's id, interned.
    pub(crate) fn ty_id(&mut self, ty: &Arc<str>) -> TyId {
        if let Some(&id) = self.ty_ids.get(ty) {
            return id;
        }
        let id = self.tys.len() as TyId;
        self.tys.push(ty.clone());
        self.ty_ids.insert(ty.clone(), id);
        id
    }

    pub(crate) fn ty(&self, id: TyId) -> Option<Arc<str>> {
        (id != NONE).then(|| self.tys[id as usize].clone())
    }

    pub(crate) fn class(&mut self, class: Class) -> ClassId {
        if let Some(&id) = self.classes.ids.get(&class) {
            return id;
        }
        let id = self.classes.classes.len() as ClassId;
        self.classes.classes.push(class.clone());
        self.classes.ids.insert(class, id);
        id
    }

    pub(crate) fn class_info(&self, class: ClassId) -> &Class {
        &self.classes.classes[class as usize]
    }

    /// The class of a function pointer.
    pub(crate) fn code_class(&mut self) -> ClassId {
        self.class(Class {
            to: Pointee::Function,
            konst: false,
        })
    }

    pub(crate) fn void_class(&mut self) -> ClassId {
        self.class(Class {
            to: Pointee::Void,
            konst: false,
        })
    }

    /// Whether a library sees the layout of every record: php-src's code
    /// the catalog does not read and the code loaded while running do.
    fn sees_every_record(&self, library: LibId) -> bool {
        let name = &*self.libraries.names[library as usize];
        name == crate::libraries::UNREAD || library == self.libraries.loaded
    }

    /// Whether a record is the libraries' own: one a header outside
    /// php-src defines, or that nothing the build reads defines.
    fn library_record(&self, record: &Arc<str>) -> bool {
        let record = self.types.canonical(record);
        self.types.layout(&record).is_none() || self.libraries.records.contains_key(&record)
    }

    /// The class of a value of a type at a library's bounds; none where it
    /// holds no pointer.
    pub(crate) fn class_of(&mut self, library: LibId, ty: &Arc<str>) -> Option<ClassId> {
        let every = self.sees_every_record(library);
        self.class_of_type(every, ty)
    }

    fn class_of_type(&mut self, every: bool, ty: &Arc<str>) -> Option<ClassId> {
        if let Some(&class) = self.classes.of_type.get(&(every, ty.clone())) {
            return class;
        }
        let class = match self.types.shape(ty) {
            Shape::Pointer { function: true } => Some(self.code_class()),
            Shape::Pointer { function: false } => match self.types.pointee(ty) {
                Some(pointee) => Some(self.pointer_class(every, &pointee)),
                None => Some(self.void_class()),
            },
            Shape::Unknown => Some(self.void_class()),
            _ => None,
        };
        self.classes.of_type.insert((every, ty.clone()), class);
        class
    }

    /// The class of a pointer to a type.
    fn pointer_class(&mut self, every: bool, ty: &Arc<str>) -> ClassId {
        let to = self.pointee_of(every, ty);
        let konst = self.types.is_const(ty);
        self.class(Class { to, konst })
    }

    /// The class of a pointer to a type at a library's bounds.
    pub(crate) fn pointer_to(&mut self, library: LibId, ty: &Arc<str>) -> ClassId {
        let every = self.sees_every_record(library);
        self.pointer_class(every, ty)
    }

    fn pointee_of(&mut self, every: bool, ty: &Arc<str>) -> Pointee {
        let facts = self.types.facts(ty);
        match facts.shape {
            Shape::Void | Shape::Unknown => Pointee::Void,
            Shape::Integer if facts.size == Some(1) => Pointee::Bytes,
            Shape::Integer | Shape::Float | Shape::Vector => Pointee::Data,
            Shape::Pointer { .. } => match self.class_of_type(every, ty) {
                Some(class) => Pointee::Pointer(class),
                None => Pointee::Void,
            },
            Shape::Record(record) => {
                if every || self.library_record(&record) {
                    Pointee::Record(record)
                } else {
                    Pointee::Void
                }
            }
            Shape::Array => match self.types.element(ty) {
                Some(element) => self.pointee_of(every, &element),
                None => Pointee::Void,
            },
            Shape::Function => Pointee::Function,
        }
    }

    /// A library's places, made where first reached.
    pub(crate) fn lib_state(&mut self, library: LibId) -> &LibState {
        if !self.libs.contains_key(&library) {
            let obj = self.object(ObjKind::Library(library), None);
            let code = self.loc(obj, Off::Exact(0));
            let mut site = Site::new(None, 0, SiteKind::ByLibrary(library));
            site.given_by = Some(library);
            let site = self.new_site(site);
            self.libs.insert(
                library,
                LibState {
                    code,
                    site,
                    held: FxHashMap::default(),
                    gives: FxHashMap::default(),
                    own: FxHashMap::default(),
                },
            );
            let code_class = self.code_class();
            let calls = self.lib_held(library, code_class, Extent::One);
            self.sites[site as usize].callee = Some(calls);
        }
        &self.libs[&library]
    }

    pub(crate) fn lib_code(&mut self, library: LibId) -> LocId {
        self.lib_state(library).code
    }

    /// The functions a library may call.
    pub(crate) fn lib_calls(&mut self, library: LibId) -> NodeId {
        let code_class = self.code_class();
        self.lib_held(library, code_class, Extent::One)
    }

    /// Whether a library gives what it holds of one class where it gives
    /// another.
    fn takes(&self, gives: ClassId, held: ClassId) -> bool {
        if gives == held {
            return true;
        }
        let (gives, held) = (self.class_info(gives), self.class_info(held));
        if gives.to == Pointee::Function || held.to == Pointee::Function {
            return false;
        }
        if held.konst && !gives.konst {
            return false;
        }
        gives.to == held.to || gives.to == Pointee::Void
    }

    /// The node of what a library holds of a class, reaching that much of
    /// what each points to.
    pub(crate) fn lib_held(&mut self, library: LibId, class: ClassId, extent: Extent) -> NodeId {
        self.lib_state(library);
        if let Some(&node) = self.libs[&library].held.get(&(class, extent)) {
            return node;
        }
        let node = self.new_node(true);
        let state = self.libs.get_mut(&library).expect("made above");
        state.held.insert((class, extent), node);
        let gives: Vec<(ClassId, NodeId)> = state.gives.iter().map(|(&c, &n)| (c, n)).collect();
        let site = state.site;
        for (gives, to) in gives {
            if self.takes(gives, class) {
                self.add_edge(node, to, COPY);
            }
        }
        self.lib_access(library, node, class, extent, true, true);
        if self.class_info(class).to == Pointee::Function {
            self.add_cons(node, Cons::Call { site });
        }
        node
    }

    /// The node of what a library gives where it gives a class.
    pub(crate) fn lib_gives(&mut self, library: LibId, class: ClassId) -> NodeId {
        self.lib_state(library);
        if let Some(&node) = self.libs[&library].gives.get(&class) {
            return node;
        }
        let node = self.new_node(true);
        let state = self.libs.get_mut(&library).expect("made above");
        state.gives.insert(class, node);
        let held: Vec<(ClassId, NodeId)> = state.held.iter().map(|(&(c, _), &n)| (c, n)).collect();
        let code = state.code;
        for (held, from) in held {
            if self.takes(class, held) {
                self.add_edge(from, node, COPY);
            }
        }
        match self.class_info(class).to.clone() {
            Pointee::Function => self.add_initial(node, code),
            to => {
                self.lib_own(library, to);
            }
        }
        node
    }

    /// A library's own memory of what a class points to, made where new:
    /// an array of records or of pointers, bytes from outside the process,
    /// data, or opaque memory holding what it gives at `void`. It holds it
    /// from the start.
    fn lib_own(&mut self, library: LibId, to: Pointee) -> LocId {
        let class = self.class(Class {
            to: to.clone(),
            konst: false,
        });
        if let Some(&loc) = self.libs[&library].own.get(&class) {
            return loc;
        }
        let obj = self.object(ObjKind::LibOwn(library, class), None);
        let loc = self.loc(obj, Off::Exact(0));
        self.libs
            .get_mut(&library)
            .expect("made by the caller")
            .own
            .insert(class, loc);
        let extent = match to {
            Pointee::Record(_) | Pointee::Pointer(_) => Extent::Array,
            _ => Extent::One,
        };
        let held = self.lib_held(library, class, extent);
        self.add_initial(held, loc);
        if to == Pointee::Void {
            let any = self.loc(obj, Off::Any);
            let gives = self.lib_gives(library, class);
            self.write_loc(any, gives, 8, 0);
        }
        loc
    }

    /// A library's reads and writes through the pointers a node holds, of
    /// a class, reaching that much of what each points to.
    pub(crate) fn lib_access(
        &mut self,
        library: LibId,
        at: NodeId,
        class: ClassId,
        extent: Extent,
        read: bool,
        write: bool,
    ) {
        if at == NONE {
            return;
        }
        let info = self.class_info(class).clone();
        let write = write && !info.konst;
        match info.to {
            Pointee::Record(record) => {
                let stride = match extent {
                    Extent::Array => self.types.record_size(&record).unwrap_or(0),
                    _ => 0,
                };
                let members = self.record_members(&record);
                for member in members.iter() {
                    match member {
                        Member::Pointer(offset, ty) => {
                            let Some(member_class) = self.class_of(library, ty) else {
                                continue;
                            };
                            let tr = self.tr(Transform::linear(*offset, stride));
                            self.lib_point(library, at, tr, member_class, read, write);
                        }
                        Member::Bytes(offset, len) => {
                            if write {
                                let tr = self.tr(Transform::linear(*offset, stride));
                                self.write_outside(at, tr, *len);
                            }
                        }
                    }
                }
            }
            Pointee::Pointer(target) => {
                let tr = match extent {
                    Extent::Array => self.tr(Transform::linear(0, 8)),
                    _ => COPY,
                };
                self.lib_point(library, at, tr, target, read, write);
            }
            Pointee::Bytes => {
                if write {
                    let len = match extent {
                        Extent::Len(len) => len.min(u64::from(u32::MAX)) as u32,
                        _ => u32::MAX,
                    };
                    self.write_outside(at, COPY, len);
                }
            }
            Pointee::Void | Pointee::Data | Pointee::Function => {}
        }
    }

    /// A library's read and write of a pointer of a class at each pointer
    /// a node holds, moved.
    fn lib_point(
        &mut self,
        library: LibId,
        at: NodeId,
        tr: super::TrId,
        class: ClassId,
        read: bool,
        write: bool,
    ) {
        if read {
            let held = self.lib_held(library, class, Extent::One);
            self.add_cons(
                at,
                Cons::Load {
                    tr,
                    width: 8,
                    ctx: 0,
                    dst: held,
                },
            );
        }
        if write {
            let gives = self.lib_gives(library, class);
            self.add_cons(
                at,
                Cons::Store {
                    tr,
                    width: 8,
                    part: 0,
                    src: gives,
                },
            );
        }
    }

    /// Writes the outside value over as many bytes at each pointer a node
    /// holds, moved.
    fn write_outside(&mut self, at: NodeId, tr: super::TrId, len: u32) {
        if len == 0 {
            return;
        }
        let src = self.outside_node;
        self.add_cons(
            at,
            Cons::Store {
                tr,
                width: len,
                part: 0,
                src,
            },
        );
    }

    /// The members of a record a library reaches, nested records and
    /// arrays element by element.
    fn record_members(&mut self, record: &Arc<str>) -> Arc<[Member]> {
        if let Some(members) = self.classes.members.get(record) {
            return members.clone();
        }
        let mut members = Vec::new();
        self.members_of(record, 0, 0, &mut members);
        let members: Arc<[Member]> = members.into();
        self.classes.members.insert(record.clone(), members.clone());
        members
    }

    fn members_of(&mut self, record: &Arc<str>, at: i64, depth: u32, out: &mut Vec<Member>) {
        if depth > 16 {
            return;
        }
        let Some(layout) = self.types.layout(record) else {
            return;
        };
        for field in &layout.fields {
            let start = at + field.offset as i64;
            self.members_of_type(&field.ty, field.size, start, depth, out);
        }
    }

    fn members_of_type(
        &mut self,
        ty: &Arc<str>,
        size: Option<u64>,
        start: i64,
        depth: u32,
        out: &mut Vec<Member>,
    ) {
        let facts = self.types.facts(ty);
        match facts.shape {
            Shape::Pointer { .. } | Shape::Unknown => out.push(Member::Pointer(start, ty.clone())),
            Shape::Integer if facts.size == Some(1) => out.push(Member::Bytes(start, 1)),
            Shape::Record(record) => self.members_of(&record, start, depth + 1, out),
            Shape::Array => {
                let Some(element) = self.types.element(ty) else {
                    return;
                };
                let element_facts = self.types.facts(&element);
                let size = size.or(facts.size).filter(|&size| size > 0);
                match element_facts.shape {
                    Shape::Integer if element_facts.size == Some(1) => {
                        let len =
                            size.map_or(u32::MAX, |size| size.min(u64::from(u32::MAX)) as u32);
                        out.push(Member::Bytes(start, len));
                    }
                    Shape::Integer | Shape::Float | Shape::Vector | Shape::Void => {}
                    _ => {
                        let Some(element_size) = element_facts.size.filter(|&size| size > 0) else {
                            return;
                        };
                        // An array of unknown length is reached at its
                        // first element.
                        let count = size.map_or(1, |size| size / element_size);
                        for i in 0..count {
                            self.members_of_type(
                                &element,
                                Some(element_size),
                                start + (i * element_size) as i64,
                                depth + 1,
                                out,
                            );
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// A library's reads and writes of a global it declares, at the
    /// global's type.
    pub(crate) fn lib_global(&mut self, library: LibId, loc: LocId, ty: &Arc<str>) {
        let node = self.const_node(loc);
        let (class, extent) = match self.types.shape(ty) {
            Shape::Array => match self.types.element(ty) {
                Some(element) => (self.pointer_to(library, &element), Extent::Array),
                None => return,
            },
            _ => (self.pointer_to(library, ty), Extent::One),
        };
        self.lib_access(library, node, class, extent, true, true);
    }

    /// A class's text, for the report.
    pub(crate) fn class_name(&self, class: ClassId) -> String {
        let info = self.class_info(class);
        let to = match &info.to {
            Pointee::Void => "void".to_owned(),
            Pointee::Bytes => "bytes".to_owned(),
            Pointee::Data => "data".to_owned(),
            Pointee::Record(record) => record.to_string(),
            Pointee::Pointer(class) => self.class_name(*class),
            Pointee::Function => return "function".to_owned(),
        };
        if info.konst {
            format!("const {to} *")
        } else {
            format!("{to} *")
        }
    }
}
