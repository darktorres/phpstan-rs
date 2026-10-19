<?php
function gen(): \Generator {
    $x = (yield and $b);
    $y = (yield | 1);
    $z = (yield instanceof Foo);
    yield from 5;
}
