//! What a type's text says about its values: their kind, their size and
//! the record they are, through the program's typedefs, and what the
//! program's records hold.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::clang::Platform;
use crate::layout::Layout;
use crate::lower::{
    anonymous_record, array_bounds, function_pointer, integer_type, matching_paren, scalar_size,
    strip_qualifiers,
};
use crate::program::Program;

/// The kind of a type's values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Shape {
    Void,
    /// An integer, an enumeration or `_Bool`.
    Integer,
    Float,
    Pointer {
        function: bool,
    },
    /// A record, by the catalog's name for it.
    Record(Arc<str>),
    Array,
    Function,
    Vector,
    Unknown,
}

impl Shape {
    /// Whether a value of the shape is held whole in a register: not a
    /// record, an array or a function.
    pub(crate) fn is_scalar(&self) -> bool {
        matches!(
            self,
            Shape::Integer | Shape::Float | Shape::Pointer { .. } | Shape::Vector
        )
    }
}

/// A type's shape and size in bytes, where known.
#[derive(Debug, Clone)]
pub(crate) struct TypeFacts {
    pub(crate) shape: Shape,
    pub(crate) size: Option<u64>,
}

pub(crate) struct Types<'p> {
    program: &'p Program,
    platform: Platform,
    facts: FxHashMap<Arc<str>, TypeFacts>,
    /// Whether each record may hold a pointer: a member of pointer type, or
    /// of a record or an array that may.
    holds_pointer: FxHashMap<Arc<str>, bool>,
}

impl<'p> Types<'p> {
    pub(crate) fn new(program: &'p Program, platform: Platform) -> Types<'p> {
        Types {
            program,
            platform,
            facts: FxHashMap::default(),
            holds_pointer: FxHashMap::default(),
        }
    }

    pub(crate) fn facts(&mut self, ty: &Arc<str>) -> TypeFacts {
        if let Some(facts) = self.facts.get(ty) {
            return facts.clone();
        }
        let facts = self.classify(ty, 0);
        self.facts.insert(ty.clone(), facts.clone());
        facts
    }

    pub(crate) fn shape(&mut self, ty: &Arc<str>) -> Shape {
        self.facts(ty).shape
    }

    pub(crate) fn size(&mut self, ty: &Arc<str>) -> Option<u64> {
        self.facts(ty).size
    }

    /// An integer type's width in bits and whether it is signed, through
    /// its typedefs; none for an enumeration, whose signedness its
    /// constants decide.
    pub(crate) fn integer(&self, ty: &str) -> Option<(u32, bool)> {
        let mut text = strip_qualifiers(ty);
        for _ in 0..32 {
            if let Some(found) = integer_type(text, self.platform) {
                return Some(found);
            }
            match self.program.typedefs.get(text) {
                Some(under) if **under != *text => text = strip_qualifiers(under),
                _ => return None,
            }
        }
        None
    }

    /// The catalog's one name for a record: an unnamed record its typedef
    /// names is named by where it is declared.
    pub(crate) fn canonical(&self, record: &Arc<str>) -> Arc<str> {
        self.program
            .aliases
            .get(record)
            .cloned()
            .unwrap_or_else(|| record.clone())
    }

    pub(crate) fn layout(&self, record: &Arc<str>) -> Option<&'p Layout> {
        let program = self.program;
        program
            .records
            .get(record)
            .or_else(|| program.records.get(&self.canonical(record)))
    }

    pub(crate) fn record_size(&self, record: &Arc<str>) -> Option<u64> {
        self.layout(record).map(|layout| layout.size)
    }

    /// Whether a record's members may hold a pointer.
    pub(crate) fn record_holds_pointer(&mut self, record: &Arc<str>) -> bool {
        let record = self.canonical(record);
        if let Some(&holds) = self.holds_pointer.get(&record) {
            return holds;
        }
        // A record holding itself (through an array) holds no pointer by
        // that member alone.
        self.holds_pointer.insert(record.clone(), false);
        let mut holds = false;
        if let Some(layout) = self.layout(&record) {
            for field in &layout.fields {
                if self.type_holds_pointer(&field.ty.clone()) {
                    holds = true;
                    break;
                }
            }
        }
        self.holds_pointer.insert(record, holds);
        holds
    }

    /// Whether a value of a type may hold a pointer by its declared type:
    /// a pointer, or a record or an array of something that may.
    pub(crate) fn type_holds_pointer(&mut self, ty: &Arc<str>) -> bool {
        match self.shape(ty) {
            Shape::Pointer { .. } => true,
            Shape::Record(record) => self.record_holds_pointer(&record),
            Shape::Array => match self.element(ty) {
                Some(element) => self.type_holds_pointer(&element),
                None => true,
            },
            Shape::Unknown => true,
            _ => false,
        }
    }

    /// Whether a type is `const` itself, through its typedefs: a pointer
    /// by a qualifier after its `*`, any other type by one anywhere.
    pub(crate) fn is_const(&self, ty: &str) -> bool {
        let mut text = ty.trim();
        for _ in 0..32 {
            if text.contains('*') || text.contains('(') {
                let declarator = text.rfind(['*', ')']).map_or(text, |at| &text[at + 1..]);
                return declarator.split_whitespace().any(|word| word == "const");
            }
            if text.split_whitespace().any(|word| word == "const") {
                return true;
            }
            match self.program.typedefs.get(strip_qualifiers(text)) {
                Some(under) if **under != *strip_qualifiers(text) => text = under.trim(),
                _ => return false,
            }
        }
        false
    }

    /// An array type's element type, through its typedefs.
    pub(crate) fn element(&mut self, ty: &Arc<str>) -> Option<Arc<str>> {
        let mut text: String = ty.to_string();
        for _ in 0..32 {
            let stripped = strip_qualifiers(&text);
            if let Some(open) = stripped.find("(*") {
                let close = matching_paren(stripped, open)?;
                let inner = &stripped[open + 2..close];
                let first = inner.find('[')?;
                let close_bound = inner[first..].find(']')? + first;
                let rest = format!(
                    "{}(*{}){}",
                    &stripped[..open],
                    &inner[close_bound + 1..],
                    &stripped[close + 1..]
                );
                return Some(Arc::from(rest));
            }
            if stripped.ends_with(']') {
                let open = stripped.find('[')?;
                let close = stripped[open..].find(']')? + open;
                let rest = stripped[close + 1..].trim_start();
                let element = if rest.is_empty() {
                    stripped[..open].trim_end().to_owned()
                } else {
                    format!("{} {rest}", stripped[..open].trim_end())
                };
                return Some(Arc::from(element));
            }
            match self.program.typedefs.get(stripped) {
                Some(under) if **under != *stripped => text = under.to_string(),
                _ => return None,
            }
        }
        None
    }

    /// The type a pointer type points to, through its typedefs; none for a
    /// type that is not a pointer.
    pub(crate) fn pointee(&mut self, ty: &Arc<str>) -> Option<Arc<str>> {
        let mut text: String = ty.to_string();
        for _ in 0..32 {
            let mut stripped = strip_qualifiers(&text);
            if let Some(inner) = stripped
                .strip_prefix("_Atomic(")
                .and_then(|t| t.strip_suffix(')'))
            {
                stripped = strip_qualifiers(inner);
            }
            if let Some(inner) = stripped.strip_suffix('*') {
                return Some(Arc::from(inner.trim_end()));
            }
            if let Some(open) = stripped.find("(*")
                && stripped.find('(') == Some(open)
            {
                let close = matching_paren(stripped, open)?;
                let inner = &stripped[open + 2..close];
                if inner.contains('[') {
                    return None;
                }
                let prefix = stripped[..open].trim_end();
                let suffix = &stripped[close + 1..];
                let pointee = if inner.trim().is_empty() {
                    format!("{prefix} {}", suffix.trim_start())
                } else {
                    format!("{prefix} ({inner}){suffix}")
                };
                return Some(Arc::from(pointee));
            }
            match self.program.typedefs.get(stripped) {
                Some(under) if **under != *stripped => text = under.to_string(),
                _ => return None,
            }
        }
        None
    }

    fn classify(&self, text: &str, depth: u32) -> TypeFacts {
        let unknown = TypeFacts {
            shape: Shape::Unknown,
            size: None,
        };
        if depth > 32 {
            return unknown;
        }
        let mut text = strip_qualifiers(text);
        if let Some(inner) = text
            .strip_prefix("_Atomic(")
            .and_then(|t| t.strip_suffix(')'))
        {
            text = strip_qualifiers(inner);
        }
        if text.ends_with('*') {
            return TypeFacts {
                shape: Shape::Pointer {
                    function: function_pointer(text, &self.program.typedefs),
                },
                size: Some(8),
            };
        }
        if let Some(open) = text.find("(*") {
            // A pointer to a function or an array, or an array of them.
            let Some(close) = matching_paren(text, open) else {
                return unknown;
            };
            let inner = &text[open + 2..close];
            let function = text[close + 1..].trim_start().starts_with('(');
            return match array_bounds(inner) {
                Some(bounds) if bounds.is_empty() => TypeFacts {
                    shape: Shape::Pointer { function },
                    size: Some(8),
                },
                Some(bounds) => TypeFacts {
                    shape: Shape::Array,
                    size: Some(8 * bounds.iter().product::<u64>()),
                },
                None => TypeFacts {
                    shape: Shape::Array,
                    size: None,
                },
            };
        }
        if text.ends_with(']') {
            let Some(open) = text.rfind('[') else {
                return unknown;
            };
            let bound: Option<u64> = text[open + 1..text.len() - 1].trim().parse().ok();
            let element = self.classify(&text[..open], depth + 1);
            return TypeFacts {
                shape: Shape::Array,
                size: bound.zip(element.size).map(|(bound, size)| bound * size),
            };
        }
        if let Some(record) = anonymous_record(text) {
            let record = self.canonical(&record);
            let size = self.record_size(&record);
            return TypeFacts {
                shape: Shape::Record(record),
                size,
            };
        }
        if text.ends_with(')') && !text.starts_with("__attribute__") {
            return TypeFacts {
                shape: Shape::Function,
                size: Some(1),
            };
        }
        if let Some(vector) = text.strip_prefix("__attribute__((__vector_size__(") {
            let size = vector.split_once(" * sizeof(").and_then(|(count, rest)| {
                let (element, _) = rest.split_once(")))) ")?;
                let count: u64 = count.trim().parse().ok()?;
                Some(count * self.classify(element, depth + 1).size?)
            });
            return TypeFacts {
                shape: Shape::Vector,
                size,
            };
        }
        if text == "void" {
            return TypeFacts {
                shape: Shape::Void,
                size: Some(1),
            };
        }
        if let Some((size, _)) = scalar_size(text, self.platform) {
            let float = text.contains("_Complex")
                || matches!(
                    text,
                    "float"
                        | "double"
                        | "long double"
                        | "_Float16"
                        | "__bf16"
                        | "_Float128"
                        | "__float128"
                );
            return TypeFacts {
                shape: if float { Shape::Float } else { Shape::Integer },
                size: Some(size),
            };
        }
        if let Some(tag) = text.strip_prefix("enum ") {
            return TypeFacts {
                shape: Shape::Integer,
                size: Some(self.program.enums.get(tag.trim()).copied().unwrap_or(4)),
            };
        }
        for keyword in ["struct ", "union "] {
            if let Some(tag) = text.strip_prefix(keyword) {
                let record = self.canonical(&Arc::from(tag.trim()));
                let size = self.record_size(&record);
                return TypeFacts {
                    shape: Shape::Record(record),
                    size,
                };
            }
        }
        if let Some(&size) = self.program.enums.get(text) {
            return TypeFacts {
                shape: Shape::Integer,
                size: Some(size),
            };
        }
        match self.program.typedefs.get(text) {
            Some(under) if **under != *text => self.classify(under, depth + 1),
            _ => {
                // A record named by its typedef alone.
                let name: Arc<str> = Arc::from(text);
                if self.program.records.contains_key(&name)
                    || self.program.aliases.contains_key(&name)
                {
                    let record = self.canonical(&name);
                    let size = self.record_size(&record);
                    return TypeFacts {
                        shape: Shape::Record(record),
                        size,
                    };
                }
                unknown
            }
        }
    }
}
