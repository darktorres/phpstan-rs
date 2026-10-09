<?php
function measurePlainObjectWeak(stdClass $value): int
{
    return strlen($value);
}
