<?php
interface P {}
/** @template T of P|null = null */
class Base {
    /** @param T $parent */
    public function __construct(public ?P $parent = null) {}
}
/**
 * @template T of P|null = null
 * @extends Base<T>
 */
class Child extends Base {
    /** @param T $parent */
    public function __construct(?P $parent = null) { parent::__construct($parent); }
}
