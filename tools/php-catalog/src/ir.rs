//! The catalog's form of a C function: a control-flow graph of blocks,
//! each a list of assignments and calls ended by a jump, over expressions
//! without side effects. Short-circuit operators, conditional operators
//! and statement expressions become blocks, so that a condition is read on
//! the path it decides.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// A C type, as clang prints it with its typedefs resolved.
pub(crate) type Ty = Arc<str>;

/// A function's name as the program links it: an external function by its
/// name, a static one by the file defining it and its name (`file:name`).
pub(crate) type FnName = Arc<str>;

/// A global variable's name as the program links it, as for functions.
pub(crate) type GlobalName = Arc<str>;

pub(crate) type VarId = u32;
pub(crate) type BlockId = u32;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Var {
    pub(crate) name: Arc<str>,
    pub(crate) ty: Ty,
}

/// A struct or union member: the record's name, then the member's.
/// An unnamed record is named by where it is declared
/// (`anon@Zend/zend_types.h:330:2`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) struct Field {
    pub(crate) record: Arc<str>,
    pub(crate) name: Arc<str>,
    /// Where the member starts in the record, in bytes, where the record's
    /// layout is known.
    pub(crate) offset: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum Place {
    Var(VarId),
    Global(GlobalName),
    /// What a pointer points to.
    Deref(Box<Expr>),
    Field(Box<Place>, Field),
    /// An element of an array (`a[i]` on an array, not a pointer) or of a
    /// vector: the array, then where the element starts in it, in bytes.
    Index(Box<Place>, Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum UnOp {
    Neg,
    Not,
    BitNot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Shl,
    Shr,
    BitAnd,
    BitOr,
    BitXor,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl BinOp {
    pub(crate) fn from_opcode(opcode: &str) -> Option<BinOp> {
        Some(match opcode {
            "+" => BinOp::Add,
            "-" => BinOp::Sub,
            "*" => BinOp::Mul,
            "/" => BinOp::Div,
            "%" => BinOp::Rem,
            "<<" => BinOp::Shl,
            ">>" => BinOp::Shr,
            "&" => BinOp::BitAnd,
            "|" => BinOp::BitOr,
            "^" => BinOp::BitXor,
            "==" => BinOp::Eq,
            "!=" => BinOp::Ne,
            "<" => BinOp::Lt,
            "<=" => BinOp::Le,
            ">" => BinOp::Gt,
            ">=" => BinOp::Ge,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct Expr {
    pub(crate) kind: ExprKind,
    pub(crate) ty: Ty,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum ExprKind {
    /// The value a place holds.
    Use(Place),
    AddrOf(Place),
    Int(i128),
    Float,
    /// A string literal's bytes, without the terminating NUL.
    Str(Arc<[u8]>),
    Func(FnName),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// A pointer moved by a number of bytes: C's pointer arithmetic, its
    /// operand scaled by the size of what the pointer points to.
    Offset(Box<Expr>, Box<Expr>),
    /// A conversion to `ty`.
    Cast(Box<Expr>),
    /// The next argument a `va_list` gives (`va_arg`): the list, as a
    /// pointer to it.
    VaArg(Box<Expr>),
    /// `sizeof` of a type.
    SizeOf(Ty),
    /// `_Alignof` of a type.
    AlignOf(Ty),
    /// A value the lowering cannot give, with why.
    Unknown(Arc<str>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) enum Callee {
    Direct(FnName),
    /// Through a function pointer.
    Indirect(Expr),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) enum Stmt {
    Assign {
        dst: Place,
        src: Expr,
        line: u32,
    },
    Call {
        dst: Option<Place>,
        callee: Callee,
        args: Vec<Expr>,
        line: u32,
    },
    /// Inline assembly: may write the objects its output operands name,
    /// from the values of its operands.
    Asm {
        writes: Vec<Place>,
        reads: Vec<Expr>,
        line: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) enum Term {
    Goto(BlockId),
    Branch {
        cond: Expr,
        then: BlockId,
        els: BlockId,
    },
    /// Each case's inclusive range, then the default's block.
    Switch {
        on: Expr,
        cases: Vec<(i128, i128, BlockId)>,
        default: BlockId,
    },
    Return(Option<Expr>),
    /// Past a call that does not return, or the end of a function that
    /// returns a value without a `return`.
    Unreachable,
    /// A computed `goto`: to any block whose address is taken.
    IndirectGoto(Vec<BlockId>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Block {
    pub(crate) stmts: Vec<Stmt>,
    pub(crate) term: Term,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Function {
    pub(crate) name: FnName,
    pub(crate) file: Arc<str>,
    pub(crate) line: u32,
    pub(crate) is_variadic: bool,
    /// The parameters are the first `params` variables.
    pub(crate) params: u32,
    pub(crate) vars: Vec<Var>,
    pub(crate) ret: Ty,
    pub(crate) blocks: Vec<Block>,
    /// What the lowering could not read; a function with any runs
    /// anything.
    pub(crate) unread: Vec<Arc<str>>,
    /// The constructs lowered by their operands alone, their values
    /// unknown.
    pub(crate) approximated: Vec<Arc<str>>,
}

/// A global variable's initializer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) enum Init {
    Expr(Expr),
    /// A record's or an array's initializer list: each member or element
    /// it gives, in order; an array's other elements given by the filler,
    /// each `element` bytes long.
    List {
        items: Vec<InitItem>,
        filler: Option<Box<Init>>,
        element: Option<u64>,
    },
    /// An object left to zero.
    Zero,
}

/// A member or an element an initializer list gives.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct InitItem {
    /// The member; none for an array's element.
    pub(crate) field: Option<Field>,
    /// Where it starts in the object the list initializes, in bytes,
    /// where its layout is known.
    pub(crate) offset: Option<u64>,
    pub(crate) init: Init,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Global {
    pub(crate) name: GlobalName,
    pub(crate) ty: Ty,
    pub(crate) init: Option<Init>,
}
