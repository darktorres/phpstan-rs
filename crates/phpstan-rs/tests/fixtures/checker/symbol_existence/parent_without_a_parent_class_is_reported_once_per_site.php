<?php
final class K {
    /** @param parent $p */
    public function g($p): void {}
    public function h(parent $p): parent { return $p; }
    /** @return parent */
    public function i() { return 1; }
}
