<?php
enum Id: string { case A = 'a'; case B = 'b'; }
/** @template T of Id */
class Builtin {}
/** @extends Builtin<Id::A> */
class ArrayBuiltin extends Builtin {}

/** @template T of UnitEnum|string|int */
abstract class Lexer {}
/** @extends Lexer<Doc::T_*> */
final class Doc extends Lexer {
    public const T_NONE = 1;
    public const T_INT = 2;
}

enum Other: string { case X = 'x'; }
/** @extends Builtin<Other::X> */
class WrongBuiltin extends Builtin {}
