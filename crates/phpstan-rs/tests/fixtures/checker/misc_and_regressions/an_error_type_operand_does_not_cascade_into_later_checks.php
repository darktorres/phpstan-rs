<?php
function f(): void {
    echo $undef->foo()->bar + 1;
    strlen($undef);
}
