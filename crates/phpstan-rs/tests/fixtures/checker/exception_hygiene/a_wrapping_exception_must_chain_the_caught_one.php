<?php
final class DomainError extends \RuntimeException {}
final class NoPrevious extends \Exception { public function __construct(string $m) { parent::__construct($m); } }
function f(): void {
    try { g(); }
    catch (\LogicException $e) { throw new DomainError('lost'); }
    try { g(); }
    catch (\LogicException $e) { throw new DomainError('kept', 0, $e); }
    try { g(); }
    catch (\LogicException $e) { throw new NoPrevious('cannot chain'); }
    try { g(); }
    catch (\LogicException $e) { if (h()) { throw new DomainError('nested'); } throw $e; }
}
function g(): void {}
function h(): bool { return true; }
