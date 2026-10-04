<?php
function f(DateTime $d = new DateTime()): DateTime
{
    static $s = new ArrayObject();
    return $d;
}

const NOW = new DateTimeImmutable();
