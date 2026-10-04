<?php
function f(?string $x): string
{
    return $x ?? throw new Exception();
}
