<?php
interface Proc { public function __invoke(int $r): int; }
interface H {
    /** @param Proc|callable(int): int $callback */
    public function push(callable $callback): void;
}
interface NotInvokable {}
interface H2 {
    /** @param NotInvokable $callback */
    public function push(callable $callback): void;
}
