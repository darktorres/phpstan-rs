<?php
function f(): void {
    echo filter_input(INPUT_GET, 'foo');
    echo filter_input(INPUT_GET, 'foo', FILTER_VALIDATE_INT);
}
