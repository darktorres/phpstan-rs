<?php
class Label { public function __toString(): string { return 'x'; } }
function f(Label $l): void {
    echo sprintf('%s', $l);
    echo sprintf('%d', $l);
}
