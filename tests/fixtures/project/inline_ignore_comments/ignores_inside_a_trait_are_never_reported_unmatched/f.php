<?php
trait T {
    public function m(): void {
        $x = 1; // @phpstan-ignore-line
    }
}
